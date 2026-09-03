import { afterEach, describe, expect, it, vi } from "vitest";
import type { PreviewTarget } from "@yas-run/core";
import {
  PREVIEW_WS_MAX_FRAME_BYTES,
  PREVIEW_WS_MAX_FRAGMENTS,
  PREVIEW_WS_MAX_HANDSHAKE_BYTES,
  PREVIEW_WS_MAX_MESSAGE_BYTES,
  shimTag,
} from "../inject";

const target: PreviewTarget = {
  dest: "local",
  scheme: "http",
  host: "localhost",
  port: 3000,
};

/**
 * Run the injected shim with `window`/`navigator`/`location` of our own.
 *
 * The shim installs itself onto those globals and takes `serviceWorker` off
 * `Navigator.prototype`; handing it stand-ins keeps jsdom's real globals
 * intact so each test gets a clean set of shims.
 */
function loadShim(opts: { controller?: unknown } = {}) {
  const html = new TextDecoder().decode(shimTag(target, "sid=abc"));
  const src = html.replace(/^<script>/, "").replace(/<\/script>$/, "");

  /** The port the worker would hold, so a test can play the relay. */
  let relay: MessagePort | null = null;
  const controller =
    "controller" in opts
      ? opts.controller
      : {
          postMessage(msg: { type: string }, transfer: MessagePort[]) {
            if (msg.type === "yas-ws-open") relay = transfer[0];
          },
        };

  const win: Record<string, unknown> = {
    WebSocket: class NativeWSStub {},
    open() {},
  };
  const fn = new Function(
    "window",
    "navigator",
    "location",
    "Document",
    "Navigator",
    src,
  );
  fn(
    win,
    { serviceWorker: { controller } },
    {
      href: "https://gateway.example/p/local",
      host: "gateway.example",
      origin: "https://gateway.example",
      protocol: "https:",
    },
    class {},
    class {},
  );

  return {
    WS: win.WebSocket as new (url: string, protocols?: string[]) => WebSocket,
    get relay() {
      return relay;
    },
  };
}

/** Subscribe the way an HMR client does: after the constructor returns. */
function watch(ws: WebSocket): string[] {
  const events: string[] = [];
  ws.onopen = () => events.push("open");
  ws.onerror = () => events.push("error");
  ws.onclose = () => events.push("close");
  return events;
}

const encoder = new TextEncoder();

/** A 101 response and nothing more — enough to reach OPEN. */
const handshake = () =>
  encoder.encode(
    "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n",
  ).buffer;

function serverFrame(opcode: number, payload = new Uint8Array(0), fin = true) {
  const out = new Uint8Array(2 + payload.length);
  out[0] = (fin ? 0x80 : 0) | opcode;
  out[1] = payload.length;
  out.set(payload, 2);
  return out.buffer;
}

function wideServerFrameHeader(opcode: number, length: bigint, fin = true) {
  const out = new Uint8Array(10);
  out[0] = (fin ? 0x80 : 0) | opcode;
  out[1] = 127;
  new DataView(out.buffer).setBigUint64(2, length);
  return out.buffer;
}

function relayCloseRequest(value: unknown): boolean {
  return !!(value as { yasClose?: boolean } | null)?.yasClose;
}

function relayBytes(value: unknown): Uint8Array | null {
  if (ArrayBuffer.isView(value))
    return new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
  if (Object.prototype.toString.call(value) === "[object ArrayBuffer]")
    return new Uint8Array(value as ArrayBuffer);
  return null;
}

/**
 * A relayed socket must always end in a close event.
 *
 * Nothing else in the stack can speak for it: the worker's sentinel covers a
 * relay that dies, but a worker the browser force-terminates sends nothing,
 * and a page that never hears a close is a page whose HMR client stops
 * reconnecting for good — connectivity lost with no symptom.
 */
describe("relayed WebSocket liveness", () => {
  afterEach(() => vi.useRealTimers());

  // clearTimeout/clearInterval must be faked too, or a cancelled timer stays
  // on the fake clock and every leak assertion below passes vacuously.
  const fake = () =>
    vi.useFakeTimers({
      toFake: [
        "setTimeout",
        "clearTimeout",
        "setInterval",
        "clearInterval",
        "Date",
      ],
    });

  it("reports a close when no service worker controls the frame", async () => {
    const shim = loadShim({ controller: null });
    const ws = new shim.WS("ws://gateway.example/hmr");
    // Constructing must not dispatch: the app has not subscribed yet.
    const events = watch(ws);
    await vi.waitFor(() => expect(events).toEqual(["error", "close"]));
  });

  it("reports a close when the pipe goes quiet while open", async () => {
    fake();
    const shim = loadShim();
    const ws = new shim.WS("ws://gateway.example/hmr");
    const events = watch(ws);
    shim.relay!.postMessage(handshake());
    await vi.waitFor(() => expect(ws.readyState).toBe(1));
    await vi.advanceTimersByTimeAsync(40_000);
    expect(events).toContain("close");
  });

  it("reports a close on the worker's relay sentinel", async () => {
    const shim = loadShim();
    const ws = new shim.WS("ws://gateway.example/hmr");
    const events = watch(ws);
    shim.relay!.postMessage({ yasClosed: true });
    await vi.waitFor(() => expect(events).toContain("close"));
  });

  it("reports a close when the app closes and no reply comes", async () => {
    fake();
    const shim = loadShim();
    const ws = new shim.WS("ws://gateway.example/hmr");
    const events = watch(ws);
    shim.relay!.postMessage(handshake());
    await vi.waitFor(() => expect(ws.readyState).toBe(1));

    ws.close();
    expect(ws.readyState).toBe(2);
    // The target is gone and will never echo the close frame.
    await vi.advanceTimersByTimeAsync(30_000);
    expect(events).toContain("close");
    expect(ws.readyState).toBe(3);
  });

  it("reports a close when the app closes before the upgrade lands", async () => {
    fake();
    const shim = loadShim();
    const ws = new shim.WS("ws://gateway.example/hmr");
    const events = watch(ws);
    ws.close();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(events).toContain("close");
    expect(ws.readyState).toBe(3);
  });

  it("reports a close when the handshake stalls", async () => {
    fake();
    const shim = loadShim();
    const ws = new shim.WS("ws://gateway.example/hmr");
    const events = watch(ws);
    const relayMessages: unknown[] = [];
    shim.relay!.onmessage = (event) => relayMessages.push(event.data);
    await vi.advanceTimersByTimeAsync(20_000);
    expect(events).toContain("close");
    await vi.waitFor(() =>
      expect(relayMessages.some(relayCloseRequest)).toBe(true),
    );
  });

  it("bounds an unterminated handshake header", async () => {
    const shim = loadShim();
    const ws = new shim.WS("ws://gateway.example/hmr");
    const events = watch(ws);
    const relayMessages: unknown[] = [];
    shim.relay!.onmessage = (event) => relayMessages.push(event.data);
    shim.relay!.postMessage(
      new Uint8Array(PREVIEW_WS_MAX_HANDSHAKE_BYTES).fill(0x41).buffer,
    );
    await vi.waitFor(() => {
      expect(events).toEqual(["error", "close"]);
      expect(relayMessages.some(relayCloseRequest)).toBe(true);
    });
  });

  it("rejects oversized and unsafe u64 frame lengths from their headers", async () => {
    for (const length of [
      BigInt(PREVIEW_WS_MAX_FRAME_BYTES + 1),
      BigInt(Number.MAX_SAFE_INTEGER) + 1n,
    ]) {
      const shim = loadShim();
      const ws = new shim.WS("ws://gateway.example/hmr");
      const events = watch(ws);
      shim.relay!.postMessage(handshake());
      await vi.waitFor(() => expect(ws.readyState).toBe(1));

      // No payload follows: the declared u64 must be rejected before it is
      // converted to Number or used to grow the receive buffer.
      shim.relay!.postMessage(wideServerFrameHeader(0x2, length));
      await vi.waitFor(() => expect(events).toContain("close"));
    }
  });

  it("bounds the complete fragmented message before waiting for its payload", async () => {
    const shim = loadShim();
    const ws = new shim.WS("ws://gateway.example/hmr");
    const events = watch(ws);
    shim.relay!.postMessage(handshake());
    await vi.waitFor(() => expect(ws.readyState).toBe(1));

    shim.relay!.postMessage(serverFrame(0x1, encoder.encode("a"), false));
    shim.relay!.postMessage(
      wideServerFrameHeader(0x0, BigInt(PREVIEW_WS_MAX_MESSAGE_BYTES)),
    );
    await vi.waitFor(() => expect(events).toContain("close"));
  });

  it("bounds zero-length fragmentation while preserving valid fragments", async () => {
    const valid = loadShim();
    const validWs = new valid.WS("ws://gateway.example/hmr");
    const messages: string[] = [];
    validWs.onmessage = (event) => messages.push(event.data as string);
    valid.relay!.postMessage(handshake());
    await vi.waitFor(() => expect(validWs.readyState).toBe(1));
    valid.relay!.postMessage(serverFrame(0x1, encoder.encode("hel"), false));
    valid.relay!.postMessage(serverFrame(0x0, encoder.encode("lo")));
    await vi.waitFor(() => expect(messages).toEqual(["hello"]));
    valid.relay!.postMessage(serverFrame(0x8, new Uint8Array([0x03, 0xe8])));
    await vi.waitFor(() => expect(validWs.readyState).toBe(3));
    valid.relay!.postMessage({ yasCloseAck: true });

    const bounded = loadShim();
    const boundedWs = new bounded.WS("ws://gateway.example/hmr");
    const events = watch(boundedWs);
    bounded.relay!.postMessage(handshake());
    await vi.waitFor(() => expect(boundedWs.readyState).toBe(1));
    const frames = new Uint8Array((PREVIEW_WS_MAX_FRAGMENTS + 1) * 2);
    frames[0] = 0x1;
    for (let i = 0; i <= PREVIEW_WS_MAX_FRAGMENTS; i++) frames[i * 2 + 1] = 0;
    bounded.relay!.postMessage(frames.buffer);
    await vi.waitFor(() => expect(events).toContain("close"));
  });

  // A leaked interval keeps a frame's timers alive for every socket an app has
  // ever opened, and an HMR client reconnects for as long as the pane is open.
  it("leaves no timer running once the target closes cleanly", async () => {
    fake();
    const shim = loadShim();
    const ws = new shim.WS("ws://gateway.example/hmr");
    const events = watch(ws);
    shim.relay!.postMessage(handshake());
    await vi.waitFor(() => expect(ws.readyState).toBe(1));

    shim.relay!.postMessage(serverFrame(0x8, new Uint8Array([0x03, 0xe8])));
    await vi.waitFor(() => expect(ws.readyState).toBe(3));
    expect(events).toContain("close");
    await vi.advanceTimersByTimeAsync(60_000);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("leaves no timer running once the socket dies", async () => {
    fake();
    const shim = loadShim();
    const ws = new shim.WS("ws://gateway.example/hmr");
    shim.relay!.postMessage(handshake());
    await vi.waitFor(() => expect(ws.readyState).toBe(1));
    shim.relay!.postMessage({ yasClosed: true });
    await vi.waitFor(() => expect(ws.readyState).toBe(3));
    await vi.advanceTimersByTimeAsync(60_000);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("still delivers messages and pongs a server ping", async () => {
    fake();
    const shim = loadShim();
    const ws = new shim.WS("ws://gateway.example/hmr");
    const seen: string[] = [];
    ws.binaryType = "arraybuffer";
    ws.onmessage = (e) => seen.push(e.data as string);
    // Read from the start, so the shim's queued upgrade request is consumed
    // here rather than landing in the assertion below.
    const written: Uint8Array[] = [];
    shim.relay!.onmessage = (e) => {
      const bytes = relayBytes(e.data);
      if (bytes) written.push(bytes);
    };
    shim.relay!.postMessage(handshake());
    await vi.waitFor(() => expect(written).toHaveLength(1));
    expect(new TextDecoder().decode(written[0])).toContain(
      "Upgrade: websocket",
    );
    written.length = 0;

    shim.relay!.postMessage(serverFrame(0x1, encoder.encode("hello")));
    shim.relay!.postMessage(serverFrame(0x9));
    // One pong, masked, with an empty payload.
    await vi.waitFor(() => {
      expect(seen).toEqual(["hello"]);
      expect(written).toHaveLength(1);
    });
    expect(written[0][0]).toBe(0x8a);
    expect(ws.readyState).toBe(1);
  });
});
