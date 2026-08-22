/** A restarted worker re-acquires native Net from a live top-level App. */
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PreviewTarget } from "@yas-run/core";
import {
  PREVIEW_NET_CLOSE,
  PREVIEW_NET_DATA,
  PREVIEW_NET_END,
  PREVIEW_NET_MAX_PENDING_WRITES,
  PREVIEW_NET_OPEN,
  PREVIEW_NET_OPENED,
  PREVIEW_NET_READ,
  PREVIEW_NET_WRITE,
  PREVIEW_NET_WRITE_OK,
} from "../../previewNetProtocol";
import { pipeWebSocket } from "../index";

const target: PreviewTarget = {
  dest: "local",
  scheme: "http",
  host: "localhost",
  port: 7777,
};

function shimPort(): {
  received: unknown[];
  port: MessagePort;
  ready(): boolean;
  send(data: Uint8Array): void;
  close(flush: boolean): void;
} {
  const received: unknown[] = [];
  const port = {
    postMessage: (message: unknown) => received.push(message),
    close: () => {},
    onmessage: null,
  } as unknown as MessagePort;
  return {
    received,
    port,
    ready: () => port.onmessage !== null,
    send: (data) => {
      if (!port.onmessage) throw new Error("worker socket is not ready");
      port.onmessage({ data: new Uint8Array(data).buffer } as MessageEvent);
    },
    close: (flush) => {
      if (!port.onmessage) throw new Error("worker socket is not ready");
      port.onmessage({ data: { yasClose: true, flush } } as MessageEvent);
    },
  };
}

function closedSentinel(received: unknown[]): boolean {
  return received.some(
    (message) => !!(message as { yasClosed?: boolean } | null)?.yasClosed,
  );
}

afterEach(() => vi.unstubAllGlobals());

describe("preview WebSocket after a worker restart", () => {
  it("reports close when no top-level App can broker native YAS Net", async () => {
    vi.stubGlobal("clients", {
      matchAll: async () => [],
      get: async () => undefined,
    });
    const { received, port } = shimPort();
    await pipeWebSocket(target, port);
    expect(closedSentinel(received)).toBe(true);
  });

  it("reacquires a native stream without a passphrase or /d WebSocket", async () => {
    const opens: unknown[] = [];
    const writes: number[][] = [];
    const appMessages: string[] = [];
    let reads = 0;
    let nativePort!: MessagePort;
    const app = {
      id: "app",
      type: "window",
      frameType: "top-level",
      focused: true,
      visibilityState: "visible",
      url: "https://gateway.example/",
      postMessage(message: unknown, transfer: Transferable[]) {
        opens.push(message);
        const port = transfer[0] as MessagePort;
        nativePort = port;
        port.onmessage = (event) => {
          const value = event.data as {
            type?: string;
            id?: number;
            data?: ArrayBuffer;
          };
          appMessages.push(value.type ?? "unknown");
          if (value.type === PREVIEW_NET_WRITE) {
            writes.push([...new Uint8Array(value.data!)]);
            port.postMessage({ type: PREVIEW_NET_WRITE_OK, id: value.id });
          } else if (value.type === PREVIEW_NET_READ) {
            reads++;
            if (reads === 1) {
              const data = new Uint8Array([9, 8]);
              port.postMessage({ type: PREVIEW_NET_DATA, data: data.buffer }, [
                data.buffer,
              ]);
            }
          }
        };
        port.start();
        queueMicrotask(() =>
          port.postMessage({ type: PREVIEW_NET_OPENED, alpn: "" }),
        );
      },
    };
    vi.stubGlobal("clients", {
      matchAll: async () => [app],
      get: async () => app,
    });
    const { received, port, ready, send } = shimPort();
    const done = pipeWebSocket(target, port);
    for (let attempts = 0; attempts < 20 && !ready(); attempts++) await tick();
    send(new Uint8Array([1, 2, 3]));
    for (let attempts = 0; attempts < 20 && writes.length === 0; attempts++)
      await tick();
    expect(writes, appMessages.join(",")).toEqual([[1, 2, 3]]);
    nativePort.postMessage({ type: PREVIEW_NET_END });
    await done;

    expect(opens).toEqual([
      {
        type: PREVIEW_NET_OPEN,
        dest: "local",
        host: "localhost",
        port: 7777,
        options: { tls: false },
      },
    ]);
    expect(received).toContainEqual(new Uint8Array([9, 8]).buffer);
    expect(closedSentinel(received)).toBe(true);
    expect(globalThis.WebSocket).toBeDefined();
  });

  it("acknowledges shim abandonment after closing the native Net stream", async () => {
    const appMessages: string[] = [];
    let writes = 0;
    const app = {
      id: "app",
      type: "window",
      frameType: "top-level",
      focused: true,
      visibilityState: "visible",
      url: "https://gateway.example/",
      postMessage(_message: unknown, transfer: Transferable[]) {
        const nativePort = transfer[0] as MessagePort;
        nativePort.onmessage = (event) => {
          const value = event.data as {
            type?: string;
            id?: number;
          };
          appMessages.push(value.type ?? "unknown");
          if (value.type === PREVIEW_NET_WRITE) {
            writes++;
            nativePort.postMessage({
              type: PREVIEW_NET_WRITE_OK,
              id: value.id,
            });
          }
          // Leave PREVIEW_NET_READ pending. Closing BrokeredNetStream must
          // reject it and release pipeWebSocket without target cooperation.
        };
        nativePort.start();
        queueMicrotask(() =>
          nativePort.postMessage({ type: PREVIEW_NET_OPENED, alpn: "" }),
        );
      },
    };
    vi.stubGlobal("clients", {
      matchAll: async () => [app],
      get: async () => app,
    });
    const shim = shimPort();
    const done = pipeWebSocket(target, shim.port);
    for (let attempts = 0; attempts < 20 && !shim.ready(); attempts++)
      await tick();
    shim.send(new Uint8Array([1, 2, 3]));
    for (let attempts = 0; attempts < 20 && writes === 0; attempts++)
      await tick();
    shim.close(true);
    await done;
    await tick();

    expect(appMessages).toContain(PREVIEW_NET_CLOSE);
    expect(shim.received).toContainEqual({ yasCloseAck: true });
    expect(closedSentinel(shim.received)).toBe(false);
  });

  it("closes when a page floods writes while native Net withholds credit", async () => {
    const appMessages: string[] = [];
    const app = {
      id: "app",
      type: "window",
      frameType: "top-level",
      focused: true,
      visibilityState: "visible",
      url: "https://gateway.example/",
      postMessage(_message: unknown, transfer: Transferable[]) {
        const nativePort = transfer[0] as MessagePort;
        nativePort.onmessage = (event) => {
          const value = event.data as { type?: string };
          appMessages.push(value.type ?? "unknown");
          // Deliberately never acknowledge writes and never answer reads.
        };
        nativePort.start();
        queueMicrotask(() =>
          nativePort.postMessage({ type: PREVIEW_NET_OPENED, alpn: "" }),
        );
      },
    };
    vi.stubGlobal("clients", {
      matchAll: async () => [app],
      get: async () => app,
    });
    const shim = shimPort();
    const done = pipeWebSocket(target, shim.port);
    for (let attempts = 0; attempts < 20 && !shim.ready(); attempts++)
      await tick();
    await tick();
    shim.send(new Uint8Array([0]));
    await tick();
    for (let index = 1; index <= PREVIEW_NET_MAX_PENDING_WRITES; index++)
      shim.send(new Uint8Array([index & 0xff]));

    await done;
    await tick();
    expect(appMessages).toContain(PREVIEW_NET_WRITE);
    expect(appMessages).toContain(PREVIEW_NET_CLOSE);
    expect(shim.received).toContainEqual({ yasCloseAck: true });
  });
});

async function tick(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
}
