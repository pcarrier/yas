// @vitest-environment node
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { webcrypto } from "node:crypto";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { UplinkEvents, YasNoiseTransport } from "../transports/noise";
import { parseUplinkUri, YasUplinkTransport } from "../transports/uplink";
import {
  generateUplinkKeyPair,
  uplinkPublicKey,
} from "../transports/uplink-crypto";

beforeAll(() => vi.stubGlobal("crypto", webcrypto));
const PRIVATE = "3iAcr2-GUIpQfQsOaabe-eD6uK8O53exaB9pKnVfotI";
const PUBLIC = "XRb2vVZJepyosoYqoXX24-lMYpkj9SekAq8ViT7RC1Q";
const url = `uplink:https://control.invalid/base#token=routing&server=${PUBLIC}`;
const binary =
  process.env.YAS_UPLINK_INTEROP_BIN ??
  fileURLToPath(
    new URL(
      "../../../../target/debug/examples/uplink-interop",
      import.meta.url,
    ),
  );

it("keeps the identity and pin out of the control-plane request", () => {
  const parsed = parseUplinkUri(url, PRIVATE);
  expect(parsed.attach).toBe("https://control.invalid/base/attach");
  expect(parsed.token).toBe("routing");
  expect(parsed.identity).toBe(PRIVATE);
  for (const invalid of [
    url + "&server=x",
    url + "&unknown=x",
    url.replace("https:", "http:"),
    url.replace("control.invalid", "user@control.invalid"),
    url.replace("/base#", "/base?q=1#"),
    url.replace("token=routing", "token="),
    url.replace("token=routing", "token=%00"),
  ])
    expect(() => parseUplinkUri(invalid, PRIVATE)).toThrow();
  expect(() => parseUplinkUri(url)).toThrow();
  expect(() => parseUplinkUri(url, "sensitive-invalid-private-key")).toThrow(
    /43 characters/,
  );
});

it("rejects routing authentication without leaking credentials into errors", async () => {
  const fetch = vi
    .fn()
    .mockResolvedValue(
      new Response("untrusted secret reflection", { status: 401 }),
    );
  vi.stubGlobal("fetch", fetch);
  const transport = new YasUplinkTransport(url, PRIVATE, { reconnect: false });
  transport.connect();
  await vi.waitFor(() => expect(transport.status).toBe("error"));
  expect(transport.authRejected).toBe(true);
  expect(transport.lastError).not.toContain(PRIVATE);
  expect(transport.lastError).not.toContain("secret reflection");
  const [address, options] = fetch.mock.calls[0]!;
  expect(address).toBe("https://control.invalid/base/attach");
  expect(options.headers.authorization).toBe("Bearer routing");
  expect(options.redirect).toBe("error");
  expect(JSON.stringify(fetch.mock.calls)).not.toContain(PRIVATE);
  expect(JSON.stringify(fetch.mock.calls)).not.toContain(PUBLIC);
  transport.close();
  vi.unstubAllGlobals();
  vi.stubGlobal("crypto", webcrypto);
});

class StdioCarrier extends UplinkEvents {
  child: ChildProcessWithoutNullStreams | null = null;
  capture: Uint8Array[] = [];
  tamper = false;
  exit: Promise<number | null> = Promise.resolve(null);
  constructor(
    private allowed: string,
    private replyAndClose = false,
  ) {
    super();
  }
  connect(): void {
    if (this.child) return;
    const child = spawn(
      binary,
      this.replyAndClose ? ["--reply-and-close"] : [],
      {
        env: {
          ...process.env,
          YAS_UPLINK_IDENTITY: PRIVATE,
          YAS_UPLINK_TEST_CLIENT: this.allowed,
        },
        stdio: "pipe",
      },
    );
    this.child = child;
    child.stdin.on("error", () => {});
    child.stderr.resume();
    this.exit = new Promise((resolve) =>
      child.once("close", (code) => {
        this.child = null;
        this.setStatus("disconnected");
        resolve(code);
      }),
    );
    child.stdout.on("data", (chunk: Buffer) => {
      // Deliberately split length prefixes, handshake messages, and records.
      for (let offset = 0; offset < chunk.length; offset += 113)
        this.emit(
          "message",
          new Uint8Array(chunk.subarray(offset, offset + 113)),
        );
    });
    this.setStatus("connected");
  }
  send(data: Uint8Array): void {
    const bytes = data.slice();
    this.capture.push(bytes.slice());
    if (this.tamper) {
      bytes[bytes.length - 1] ^= 1;
      this.tamper = false;
    }
    this.child?.stdin.write(bytes);
  }
  close(): void {
    this.child?.stdin.end();
  }
  suspend(): void {
    this.close();
  }
}
const active: YasNoiseTransport[] = [];
afterEach(() => {
  for (const transport of active.splice(0)) transport.close();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.stubGlobal("crypto", webcrypto);
});

function mockUplinkWorker(create: () => StdioCarrier | null) {
  const fetch = vi.fn(
    async () =>
      new Response(JSON.stringify({ ws: "wss://worker.invalid/opaque" })),
  );
  const workers: StdioCarrier[] = [];
  const sockets: MockWebSocket[] = [];
  const tokens: unknown[] = [];
  class MockWebSocket {
    binaryType = "blob";
    bufferedAmount = 0;
    onopen: (() => void) | null = null;
    onmessage: ((event: { data: unknown }) => void) | null = null;
    onclose: (() => void) | null = null;
    onerror: (() => void) | null = null;
    readonly carrier = create();
    constructor(address: string) {
      expect(address).toBe("wss://worker.invalid/opaque");
      sockets.push(this);
      if (this.carrier) workers.push(this.carrier);
      this.carrier?.addEventListener("message", (message) => {
        const bytes =
          message instanceof Uint8Array ? message : new Uint8Array(message);
        this.onmessage?.({ data: bytes.slice().buffer });
      });
      this.carrier?.addEventListener("statuschange", (status) => {
        if (status === "disconnected") this.onclose?.();
      });
      queueMicrotask(() => this.onopen?.());
    }
    send(data: unknown): void {
      if (typeof data === "string") {
        tokens.push(data);
        this.carrier?.connect();
        queueMicrotask(() => this.onmessage?.({ data: "ok" }));
      } else this.carrier?.send(data as Uint8Array);
    }
    close(): void {
      this.carrier?.close();
    }
  }
  vi.stubGlobal("fetch", fetch);
  vi.stubGlobal("WebSocket", MockWebSocket);
  return { fetch, workers, sockets, tokens };
}

class SilentCarrier extends UplinkEvents {
  sent: Uint8Array[] = [];
  closed = false;
  connect(): void {
    this.setStatus("connected");
  }
  send(data: Uint8Array): void {
    this.sent.push(data.slice());
  }
  close(): void {
    this.closed = true;
    this.setStatus("closed");
  }
  deliver(data: Uint8Array): void {
    this.emit("message", data);
  }
}

it("bounds unauthenticated buffering and closes carriers without suspend support", async () => {
  const carrier = new SilentCarrier();
  const transport = new YasNoiseTransport(carrier, PRIVATE, PUBLIC);
  active.push(transport);
  transport.connect();
  carrier.deliver(new Uint8Array(65537));
  expect(transport.status).toBe("error");
  expect(carrier.closed).toBe(true);
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(carrier.sent).toHaveLength(0);
});

it("times out stalled handshakes and cancels pending crypto on close", async () => {
  const carrier = new SilentCarrier();
  const transport = new YasNoiseTransport(carrier, PRIVATE, PUBLIC, {
    connectTimeoutMs: 20,
  });
  active.push(transport);
  transport.connect();
  await vi.waitFor(() => expect(transport.status).toBe("error"));
  expect(carrier.closed).toBe(true);
  expect(transport.lastError).toMatch(/timed out/);

  const canceled = new SilentCarrier();
  const wrapper = new YasNoiseTransport(canceled, PRIVATE, PUBLIC);
  active.push(wrapper);
  wrapper.connect();
  wrapper.close();
  await vi.waitFor(() => expect(canceled.closed).toBe(true));
  expect(canceled.sent).toHaveLength(0);
});

it("backs off after Noise timeouts and cancels pending retries on suspend", async () => {
  vi.useFakeTimers();
  const { fetch } = mockUplinkWorker(() => null);
  const transport = new YasUplinkTransport(url, PRIVATE, {
    connectTimeoutMs: 20,
    reconnectDelay: 10,
    reconnectBackoff: 2,
  });
  active.push(transport);
  transport.connect();
  await vi.advanceTimersByTimeAsync(20);
  expect(transport.status).toBe("error");
  expect(transport.authRejected).toBe(false);
  expect(transport.lastError).toMatch(/timed out/);
  expect(fetch).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(10);
  expect(fetch).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(39);
  expect(fetch).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(1);
  expect(fetch).toHaveBeenCalledTimes(3);
  await vi.advanceTimersByTimeAsync(20);
  transport.suspend();
  await vi.advanceTimersByTimeAsync(1000);
  expect(fetch).toHaveBeenCalledTimes(3);
  expect(transport.status).toBe("disconnected");
});

it.each(["disabled", "closed", "rejected"] as const)(
  "does not retry when %s",
  async (mode) => {
    vi.useFakeTimers();
    const { fetch } = mockUplinkWorker(() => null);
    if (mode === "rejected")
      fetch.mockImplementation(
        async () => new Response("denied", { status: 403 }),
      );
    const transport = new YasUplinkTransport(url, PRIVATE, {
      connectTimeoutMs: 20,
      reconnectDelay: 10,
      reconnect: mode !== "disabled",
    });
    active.push(transport);
    transport.connect();
    await vi.advanceTimersByTimeAsync(20);
    expect(transport.status).toBe("error");
    if (mode === "closed") transport.close();
    await vi.advanceTimersByTimeAsync(1000);
    expect(fetch).toHaveBeenCalledTimes(1);
    expect(transport.authRejected).toBe(mode === "rejected");
  },
);

describe.skipIf(!existsSync(binary))("browser WebCrypto to native Snow", () => {
  it("discards old decryption completions after an explicit reconnect", async () => {
    const client = await generateUplinkKeyPair();
    const { workers } = mockUplinkWorker(
      () => new StdioCarrier(client.publicKey),
    );
    const transport = new YasUplinkTransport(url, client.privateKey, {
      reconnect: false,
    });
    active.push(transport);
    const received: Uint8Array[] = [];
    transport.addEventListener("message", (data) =>
      received.push(new Uint8Array(data)),
    );
    transport.connect();
    await vi.waitFor(() => expect(transport.status).toBe("connected"));
    const entered = Promise.withResolvers<void>();
    const resume = Promise.withResolvers<void>();
    const decrypt = crypto.subtle.decrypt.bind(crypto.subtle);
    vi.spyOn(crypto.subtle, "decrypt").mockImplementation(async (...args) => {
      const result = await decrypt(...args);
      entered.resolve();
      await resume.promise;
      return result;
    });
    try {
      transport.send(new TextEncoder().encode("old session"));
      await entered.promise;
      transport.reconnect();
      await vi.waitFor(() => expect(workers).toHaveLength(2));
    } finally {
      resume.resolve();
    }
    await vi.waitFor(() => expect(transport.status).toBe("connected"));
    expect(received).toHaveLength(0);
    const payload = new TextEncoder().encode("new session");
    transport.send(payload);
    await vi.waitFor(() =>
      expect(Buffer.concat(received)).toEqual(Buffer.from(payload)),
    );
  });

  it.each([false, true])(
    "drains data and FIN before EOF or retry (reconnect=%s)",
    async (reconnect) => {
      const client = await generateUplinkKeyPair();
      const { fetch, workers } = mockUplinkWorker(
        () => new StdioCarrier(client.publicKey, true),
      );
      const transport = new YasUplinkTransport(url, client.privateKey, {
        reconnect,
        reconnectDelay: 1,
      });
      active.push(transport);
      const received: Uint8Array[] = [];
      const states: string[] = [];
      transport.addEventListener("message", (data) =>
        received.push(new Uint8Array(data)),
      );
      transport.addEventListener("statuschange", (status) =>
        states.push(status),
      );
      transport.connect();
      await vi.waitFor(() => expect(transport.status).toBe("connected"));

      // Hold a real decryption completion until the native producer has sent FIN
      // and closed the carrier. No timing assumption about WebCrypto's speed.
      const entered = Promise.withResolvers<void>();
      const resume = Promise.withResolvers<void>();
      const decrypt = crypto.subtle.decrypt.bind(crypto.subtle);
      vi.spyOn(crypto.subtle, "decrypt").mockImplementation(async (...args) => {
        const result = await decrypt(...args);
        entered.resolve();
        await resume.promise;
        return result;
      });
      const payload = new TextEncoder().encode("final command result");
      try {
        transport.send(payload);
        await entered.promise;
        expect(await workers[0]!.exit).toBe(0);
        await new Promise((resolve) => setTimeout(resolve, 20));
        expect(received).toHaveLength(0);
        expect(fetch).toHaveBeenCalledTimes(1);
        expect(transport.status).toBe("connected");
      } finally {
        resume.resolve();
      }
      await vi.waitFor(() =>
        expect(Buffer.concat(received)).toEqual(Buffer.from(payload)),
      );
      expect(states).toContain("disconnected");
      expect(states).not.toContain("error");
      expect(transport.lastError).toBeNull();
      if (reconnect) {
        await vi.waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
        await vi.waitFor(() => expect(transport.status).toBe("connected"));
        expect(workers[0]!.capture[0]).not.toEqual(workers[1]!.capture[0]);
      } else expect(transport.status).toBe("disconnected");
    },
  );

  it("can reconnect manually after a clean remote FIN", async () => {
    const client = await generateUplinkKeyPair();
    const { workers } = mockUplinkWorker(
      () => new StdioCarrier(client.publicKey, true),
    );
    const transport = new YasUplinkTransport(url, client.privateKey, {
      reconnect: false,
    });
    active.push(transport);
    const received: Uint8Array[] = [];
    transport.addEventListener("message", (data) =>
      received.push(new Uint8Array(data)),
    );
    transport.connect();
    const payload = new TextEncoder().encode("reply then FIN");
    for (let attempt = 0; attempt < 2; attempt++) {
      await vi.waitFor(() => expect(transport.status).toBe("connected"));
      transport.send(payload);
      await vi.waitFor(() =>
        expect(
          transport.status,
          transport.lastError ?? "no transport error",
        ).toBe("disconnected"),
      );
      expect(await workers[attempt]!.exit).toBe(0);
      expect(transport.lastError).toBeNull();
      if (attempt === 0) transport.reconnect();
    }
    expect(Buffer.concat(received)).toEqual(Buffer.concat([payload, payload]));
    expect(workers[0]!.capture[0]).not.toEqual(workers[1]!.capture[0]);
  });

  it("rejects EOF in a partial record without claiming authentication rejection", async () => {
    const client = await generateUplinkKeyPair();
    const { sockets, workers } = mockUplinkWorker(
      () => new StdioCarrier(client.publicKey),
    );
    const transport = new YasUplinkTransport(url, client.privateKey, {
      reconnect: false,
    });
    active.push(transport);
    const received = vi.fn();
    transport.addEventListener("message", received);
    transport.connect();
    await vi.waitFor(() => expect(transport.status).toBe("connected"));
    sockets[0]!.onmessage?.({ data: new Uint8Array([0, 18, 0]).buffer });
    sockets[0]!.onclose?.();
    await vi.waitFor(() => expect(transport.status).toBe("error"));
    expect(transport.lastError).toMatch(/truncated/);
    expect(transport.authRejected).toBe(false);
    expect(received).not.toHaveBeenCalled();
    await workers[0]!.exit;
  });

  it("recovers automatically when only the first Noise handshake stalls", async () => {
    const client = await generateUplinkKeyPair();
    let attempt = 0;
    const { fetch } = mockUplinkWorker(() =>
      attempt++ === 0 ? null : new StdioCarrier(client.publicKey),
    );
    const transport = new YasUplinkTransport(url, client.privateKey, {
      connectTimeoutMs: 200,
      reconnectDelay: 1,
    });
    active.push(transport);
    transport.connect();
    await vi.waitFor(() => expect(transport.status).toBe("connected"), {
      timeout: 2000,
    });
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(transport.authRejected).toBe(false);
    expect(transport.lastError).toBeNull();
  });

  it("routes a browser WebSocket attachment to native Noise and reconnects with fresh sessions", async () => {
    const client = await generateUplinkKeyPair();
    const { fetch, workers, tokens } = mockUplinkWorker(
      () => new StdioCarrier(client.publicKey),
    );
    const transport = new YasUplinkTransport(url, client.privateKey, {
      reconnect: false,
    });
    active.push(transport);
    const received: Uint8Array[] = [];
    transport.addEventListener("message", (bytes) =>
      received.push(new Uint8Array(bytes)),
    );
    const payload = new TextEncoder().encode("browser-to-native".repeat(3000));
    transport.connect();
    for (let attempt = 0; attempt < 2; attempt++) {
      await vi.waitFor(() => expect(transport.status).toBe("connected"));
      transport.send(payload);
      await vi.waitFor(() =>
        expect(Buffer.concat(received)).toEqual(Buffer.from(payload)),
      );
      received.length = 0;
      if (attempt === 0) {
        transport.suspend();
        await workers[0]!.exit;
        transport.reconnect();
      }
    }
    expect(fetch).toHaveBeenCalledTimes(2);
    expect(tokens).toEqual(["routing", "routing"]);
    expect(workers[0]!.capture[0]).not.toEqual(workers[1]!.capture[0]);
    expect(JSON.stringify(fetch.mock.calls)).not.toContain(client.privateKey);
    transport.close();
    expect(await workers[1]!.exit).toBe(0);
  });
  it("exchanges fragmented large records and authenticates closure", async () => {
    const client = await generateUplinkKeyPair();
    const carrier = new StdioCarrier(client.publicKey);
    const transport = new YasNoiseTransport(carrier, client.privateKey, PUBLIC);
    active.push(transport);
    const received: Uint8Array[] = [];
    transport.addEventListener("message", (data) =>
      received.push(
        data instanceof Uint8Array ? data.slice() : new Uint8Array(data),
      ),
    );
    transport.connect();
    await vi.waitFor(() => expect(transport.status).toBe("connected"));
    const payload = new TextEncoder().encode(
      "private browser audio and YAS bytes".repeat(4096),
    );
    transport.send(payload);
    await vi.waitFor(() =>
      expect(received.reduce((n, p) => n + p.length, 0)).toBe(payload.length),
    );
    expect(Buffer.concat(received)).toEqual(Buffer.from(payload));
    expect(
      Buffer.concat(carrier.capture).includes(
        Buffer.from("private browser audio"),
      ),
    ).toBe(false);
    transport.close();
    expect(await carrier.exit).toBe(0);
  });
  it("rejects unknown clients before releasing any YAS data", async () => {
    const allowed = await generateUplinkKeyPair(),
      attacker = await generateUplinkKeyPair();
    const carrier = new StdioCarrier(allowed.publicKey);
    const transport = new YasNoiseTransport(
      carrier,
      attacker.privateKey,
      PUBLIC,
    );
    active.push(transport);
    const connected = vi.fn();
    transport.addEventListener("statuschange", (state) => {
      if (state === "connected") connected();
    });
    transport.connect();
    expect(await carrier.exit).not.toBe(0);
    expect(connected).not.toHaveBeenCalled();
  });
  it("rejects the wrong server pin", async () => {
    const client = await generateUplinkKeyPair();
    const carrier = new StdioCarrier(client.publicKey);
    const transport = new YasNoiseTransport(
      carrier,
      client.privateKey,
      await uplinkPublicKey(client.privateKey),
    );
    active.push(transport);
    transport.connect();
    expect(await carrier.exit).not.toBe(0);
    expect(transport.status).not.toBe("connected");
  });
  it("fails on ciphertext tampering", async () => {
    const client = await generateUplinkKeyPair();
    const carrier = new StdioCarrier(client.publicKey);
    const transport = new YasNoiseTransport(carrier, client.privateKey, PUBLIC);
    active.push(transport);
    const received = vi.fn();
    transport.addEventListener("message", received);
    transport.connect();
    await vi.waitFor(() => expect(transport.status).toBe("connected"));
    carrier.tamper = true;
    transport.send(new TextEncoder().encode("forged command"));
    expect(await carrier.exit).not.toBe(0);
    expect(received).not.toHaveBeenCalled();
  });
});
