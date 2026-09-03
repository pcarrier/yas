import { afterEach, describe, expect, it, vi } from "vitest";
import {
  PREVIEW_NET_CLOSE,
  PREVIEW_NET_DATA,
  PREVIEW_NET_END,
  PREVIEW_NET_ERROR,
  PREVIEW_NET_OPENED,
  PREVIEW_NET_READ,
  PREVIEW_NET_SHUTDOWN_WRITE,
  PREVIEW_NET_WRITE,
  PREVIEW_NET_WRITE_OK,
  type PreviewNetOpenMessage,
} from "../../previewNetProtocol";
import { PreviewNetBroker } from "../conn";

interface Harness {
  broker: PreviewNetBroker;
  requests: PreviewNetOpenMessage[];
  appPorts: MessagePort[];
}

function harness(
  start: (port: MessagePort, request: PreviewNetOpenMessage) => void = (
    port,
  ) => {
    queueMicrotask(() =>
      port.postMessage({ type: PREVIEW_NET_OPENED, alpn: "http/1.1" }),
    );
  },
): Harness {
  const requests: PreviewNetOpenMessage[] = [];
  const appPorts: MessagePort[] = [];
  const broker = new PreviewNetBroker(async (request) => {
    requests.push(request);
    const channel = new MessageChannel();
    appPorts.push(channel.port2);
    channel.port2.start();
    start(channel.port2, request);
    return channel.port1;
  });
  return { broker, requests, appPorts };
}

afterEach(() => vi.useRealTimers());

describe("native preview Net broker", () => {
  it("asks the App for the exact destination and negotiates ALPN", async () => {
    const { broker, requests } = harness();
    const stream = await broker.open("build", "127.0.0.1", 8443, {
      tls: true,
      alpn: ["http/1.1"],
    });
    await expect(stream.opened).resolves.toBe("http/1.1");
    expect(requests).toEqual([
      {
        type: "yas-preview-net-open",
        dest: "build",
        host: "127.0.0.1",
        port: 8443,
        options: { tls: true, alpn: ["http/1.1"] },
      },
    ]);
  });

  it("serializes writes until the App acknowledges native Transfer credit", async () => {
    const writes: Array<{ id: number; bytes: number[] }> = [];
    let app!: MessagePort;
    const { broker } = harness((port) => {
      app = port;
      port.onmessage = (event) => {
        const value = event.data as {
          type: string;
          id: number;
          data: ArrayBuffer;
        };
        if (value.type === PREVIEW_NET_WRITE)
          writes.push({
            id: value.id,
            bytes: [...new Uint8Array(value.data)],
          });
      };
      queueMicrotask(() =>
        port.postMessage({ type: PREVIEW_NET_OPENED, alpn: "" }),
      );
    });
    const stream = await broker.open("local", "localhost", 8080);
    await stream.opened;
    const first = stream.write(new Uint8Array([1, 2]));
    const second = stream.write(new Uint8Array([3]));
    await vi.waitFor(() => expect(writes).toEqual([{ id: 1, bytes: [1, 2] }]));
    app.postMessage({ type: PREVIEW_NET_WRITE_OK, id: 1 });
    await first;
    await vi.waitFor(() =>
      expect(writes).toEqual([
        { id: 1, bytes: [1, 2] },
        { id: 2, bytes: [3] },
      ]),
    );
    app.postMessage({ type: PREVIEW_NET_WRITE_OK, id: 2 });
    await second;
  });

  it("pulls one read chunk at a time and stops at END", async () => {
    let reads = 0;
    const { broker } = harness((port) => {
      port.onmessage = (event) => {
        if (event.data?.type !== PREVIEW_NET_READ) return;
        reads++;
        if (reads === 1) {
          const data = new Uint8Array([4, 5]);
          port.postMessage({ type: PREVIEW_NET_DATA, data: data.buffer }, [
            data.buffer,
          ]);
        } else port.postMessage({ type: PREVIEW_NET_END });
      };
      queueMicrotask(() =>
        port.postMessage({ type: PREVIEW_NET_OPENED, alpn: "" }),
      );
    });
    const stream = await broker.open("local", "localhost", 8080);
    const source = stream.read();
    await expect(source.next()).resolves.toEqual({
      done: false,
      value: new Uint8Array([4, 5]),
    });
    await expect(source.next()).resolves.toEqual({
      done: true,
      value: undefined,
    });
    expect(reads).toBe(2);
  });

  it("forwards half-close and full close without a gateway WebSocket", async () => {
    const messages: string[] = [];
    const { broker } = harness((port) => {
      port.onmessage = (event) => messages.push(event.data?.type);
      queueMicrotask(() =>
        port.postMessage({ type: PREVIEW_NET_OPENED, alpn: "" }),
      );
    });
    const stream = await broker.open("local", "localhost", 8080);
    await stream.opened;
    stream.shutdownWrite();
    stream.close();
    await vi.waitFor(() =>
      expect(messages).toEqual([PREVIEW_NET_SHUTDOWN_WRITE, PREVIEW_NET_CLOSE]),
    );
    expect(vi.isMockFunction(globalThis.WebSocket)).toBe(false);
  });

  it("preserves native open failures", async () => {
    const { broker } = harness((port) => {
      queueMicrotask(() =>
        port.postMessage({
          type: PREVIEW_NET_ERROR,
          detail: "connection refused",
        }),
      );
    });
    const stream = await broker.open("local", "localhost", 9);
    await expect(stream.opened).rejects.toThrow("connection refused");
  });
});
