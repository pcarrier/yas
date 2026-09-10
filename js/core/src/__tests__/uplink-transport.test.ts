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
  constructor(private allowed: string) {
    super();
  }
  connect(): void {
    if (this.child) return;
    const child = spawn(binary, [], {
      env: {
        ...process.env,
        YAS_UPLINK_IDENTITY: PRIVATE,
        YAS_UPLINK_TEST_CLIENT: this.allowed,
      },
      stdio: "pipe",
    });
    this.child = child;
    child.stdin.on("error", () => {});
    child.stderr.resume();
    this.exit = new Promise((resolve) =>
      child.once("exit", (code) => {
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
  vi.unstubAllGlobals();
  vi.stubGlobal("crypto", webcrypto);
});

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

describe.skipIf(!existsSync(binary))("browser WebCrypto to native Snow", () => {
  it("routes a browser WebSocket attachment to native Noise and reconnects with fresh sessions", async () => {
    const client = await generateUplinkKeyPair();
    const fetch = vi
      .fn()
      .mockImplementation(
        async () =>
          new Response(JSON.stringify({ ws: "wss://worker.invalid/opaque" })),
      );
    const workers: StdioCarrier[] = [];
    const tokens: unknown[] = [];
    class MockWebSocket {
      binaryType = "blob";
      bufferedAmount = 0;
      onopen: (() => void) | null = null;
      onmessage: ((event: { data: unknown }) => void) | null = null;
      onclose: (() => void) | null = null;
      onerror: (() => void) | null = null;
      readonly carrier = new StdioCarrier(client.publicKey);
      constructor(address: string) {
        expect(address).toBe("wss://worker.invalid/opaque");
        workers.push(this.carrier);
        this.carrier.addEventListener("message", (message) => {
          const bytes =
            message instanceof Uint8Array ? message : new Uint8Array(message);
          this.onmessage?.({ data: bytes.slice().buffer });
        });
        this.carrier.addEventListener("statuschange", (status) => {
          if (status === "disconnected") this.onclose?.();
        });
        queueMicrotask(() => this.onopen?.());
      }
      send(data: unknown): void {
        if (typeof data === "string") {
          tokens.push(data);
          this.carrier.connect();
          queueMicrotask(() => this.onmessage?.({ data: "ok" }));
        } else this.carrier.send(data as Uint8Array);
      }
      close(): void {
        this.carrier.close();
      }
    }
    vi.stubGlobal("fetch", fetch);
    vi.stubGlobal("WebSocket", MockWebSocket);
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
