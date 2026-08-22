import { afterEach, describe, expect, it, vi } from "vitest";
import {
  YAS_NET_DELIVERY_PREFERENCE_NOT_APPLICABLE,
  YAS_NET_DIRECTION_DUPLEX,
  YAS_NET_DROP_NOT_APPLICABLE,
  YAS_NET_MODE_BYTE,
  YAS_NET_TLS_VERIFY_STRICT,
  YasNetFlow,
  type YasNetClient,
  type YasNetOpen,
  type YasTransfer,
} from "@yas-run/core";
import {
  PREVIEW_NET_ACCEPTED,
  PREVIEW_NET_CLOSE,
  PREVIEW_NET_DATA,
  PREVIEW_NET_END,
  PREVIEW_NET_ERROR,
  PREVIEW_NET_MAX_PENDING_WRITES,
  PREVIEW_NET_OPEN,
  PREVIEW_NET_OPENED,
  PREVIEW_NET_READ,
  PREVIEW_NET_SHUTDOWN_WRITE,
  PREVIEW_NET_WRITE,
  PREVIEW_NET_WRITE_OK,
  installPreviewNetBroker,
  type PreviewNetClient,
} from "../previewNetProtocol";

class FakeServiceWorkerContainer {
  listener: ((event: MessageEvent) => void) | null = null;

  addEventListener(type: string, listener: EventListener): void {
    if (type === "message")
      this.listener = listener as unknown as (event: MessageEvent) => void;
  }

  removeEventListener(type: string, listener: EventListener): void {
    if (type === "message" && this.listener === listener) this.listener = null;
  }

  open(data: unknown, port: MessagePort): void {
    this.listener?.({ data, ports: [port] } as unknown as MessageEvent);
  }
}

function fakeFlow(chunks: readonly Uint8Array[] = []): {
  flow: YasNetFlow;
  write: ReturnType<typeof vi.fn>;
  shutdownWrite: ReturnType<typeof vi.fn>;
  closeFlow: ReturnType<typeof vi.fn>;
} {
  const write = vi.fn().mockResolvedValue(undefined);
  const shutdownWrite = vi.fn();
  const closeFlow = vi.fn().mockResolvedValue(undefined);
  let readIndex = 0;
  const transfer = {
    write,
    closeWrite: shutdownWrite,
    read: vi.fn(async () =>
      readIndex < chunks.length ? new Uint8Array(chunks[readIndex++]) : null,
    ),
  } as unknown as YasTransfer;
  const client = { closeFlow } as unknown as YasNetClient;
  return {
    flow: new YasNetFlow(
      client,
      {
        flowHandle: 0x8000_0000_0000_0007n,
        mode: YAS_NET_MODE_BYTE,
        direction: YAS_NET_DIRECTION_DUPLEX,
        selectedDelivery: YAS_NET_DELIVERY_PREFERENCE_NOT_APPLICABLE,
        maxDatagramPayload: 0,
        serverInstanceLimit: 1,
        maxMessageBytes: 0n,
        peerAddress: { kind: "tcp", host: "127.0.0.1", port: 8443 },
        negotiatedAlpn: new TextEncoder().encode("http/1.1"),
        extensions: [],
      },
      transfer,
    ),
    write,
    shutdownWrite,
    closeFlow,
  };
}

function fakeClient(flow: YasNetFlow): {
  client: PreviewNetClient;
  open: ReturnType<typeof vi.fn>;
} {
  const open = vi.fn().mockResolvedValue(flow);
  return { client: { open }, open };
}

afterEach(() => vi.unstubAllGlobals());

describe("App-owned preview Net broker", () => {
  it("resolves a destination to native Net and demand-pumps the stream", async () => {
    const worker = new FakeServiceWorkerContainer();
    vi.stubGlobal("navigator", { serviceWorker: worker });
    const native = fakeFlow([new Uint8Array([9, 8])]);
    const { client, open } = fakeClient(native.flow);
    const stop = installPreviewNetBroker((dest) =>
      dest === "remote" ? client : null,
    );
    const channel = new MessageChannel();
    const received: unknown[] = [];
    channel.port1.onmessage = (event) => received.push(event.data);
    channel.port1.start();
    worker.open(
      {
        type: PREVIEW_NET_OPEN,
        dest: "remote",
        host: "127.0.0.1",
        port: 8443,
        options: { tls: true, alpn: ["http/1.1"] },
      },
      channel.port2,
    );
    await waitFor(() =>
      received.some(
        (value) =>
          (value as { type?: string } | null)?.type === PREVIEW_NET_OPENED,
      ),
    );
    expect(received[0]).toEqual({ type: PREVIEW_NET_ACCEPTED });
    expect(open).toHaveBeenCalledOnce();
    const nativeOpen = open.mock.calls[0]![0] as YasNetOpen;
    expect(nativeOpen).toMatchObject({
      address: { kind: "tcp", host: "127.0.0.1", port: 8443 },
      deliveryPreference: YAS_NET_DELIVERY_PREFERENCE_NOT_APPLICABLE,
      dropPolicy: YAS_NET_DROP_NOT_APPLICABLE,
      tls: {
        verification: YAS_NET_TLS_VERIFY_STRICT,
        sni: "127.0.0.1",
      },
    });
    expect(nativeOpen.operationId).toHaveLength(16);
    expect(new TextDecoder().decode(nativeOpen.tls?.alpn[0])).toBe("http/1.1");

    const data = new Uint8Array([1, 2]);
    channel.port1.postMessage(
      { type: PREVIEW_NET_WRITE, id: 1, data: data.buffer },
      [data.buffer],
    );
    await waitFor(() =>
      received.some(
        (value) =>
          (value as { type?: string } | null)?.type === PREVIEW_NET_WRITE_OK,
      ),
    );
    expect(native.write).toHaveBeenCalledWith(new Uint8Array([1, 2]));

    channel.port1.postMessage({ type: PREVIEW_NET_READ });
    await waitFor(() =>
      received.some(
        (value) =>
          (value as { type?: string } | null)?.type === PREVIEW_NET_DATA,
      ),
    );
    const message = received.find(
      (value) => (value as { type?: string } | null)?.type === PREVIEW_NET_DATA,
    ) as { data: ArrayBuffer };
    expect([...new Uint8Array(message.data)]).toEqual([9, 8]);

    channel.port1.postMessage({ type: PREVIEW_NET_READ });
    await waitFor(() =>
      received.some(
        (value) =>
          (value as { type?: string } | null)?.type === PREVIEW_NET_END,
      ),
    );
    stop();
  });

  it("forwards shutdown/close and releases streams on broker cleanup", async () => {
    const worker = new FakeServiceWorkerContainer();
    vi.stubGlobal("navigator", { serviceWorker: worker });
    const native = fakeFlow();
    const { client } = fakeClient(native.flow);
    const stop = installPreviewNetBroker(() => client);
    const channel = new MessageChannel();
    channel.port1.start();
    worker.open(
      {
        type: PREVIEW_NET_OPEN,
        dest: "local",
        host: "localhost",
        port: 80,
        options: {},
      },
      channel.port2,
    );
    await tick();
    channel.port1.postMessage({ type: PREVIEW_NET_SHUTDOWN_WRITE });
    await tick();
    expect(native.shutdownWrite).toHaveBeenCalledOnce();
    channel.port1.postMessage({ type: PREVIEW_NET_CLOSE });
    await tick();
    expect(native.closeFlow).toHaveBeenCalledOnce();
    stop();
  });

  it("closes a hostile worker that pipelines writes without acknowledgements", async () => {
    const worker = new FakeServiceWorkerContainer();
    vi.stubGlobal("navigator", { serviceWorker: worker });
    const native = fakeFlow();
    native.write.mockImplementation(() => new Promise<void>(() => {}));
    const { client } = fakeClient(native.flow);
    const stop = installPreviewNetBroker(() => client);
    const channel = new MessageChannel();
    const received: unknown[] = [];
    channel.port1.onmessage = (event) => received.push(event.data);
    channel.port1.start();
    worker.open(
      {
        type: PREVIEW_NET_OPEN,
        dest: "local",
        host: "localhost",
        port: 80,
        options: {},
      },
      channel.port2,
    );
    await waitFor(() =>
      received.some(
        (value) =>
          (value as { type?: string } | null)?.type === PREVIEW_NET_OPENED,
      ),
    );

    for (let id = 1; id <= PREVIEW_NET_MAX_PENDING_WRITES + 1; id++) {
      const data = new Uint8Array([id & 0xff]);
      channel.port1.postMessage(
        { type: PREVIEW_NET_WRITE, id, data: data.buffer },
        [data.buffer],
      );
    }
    await waitFor(() => native.closeFlow.mock.calls.length !== 0);
    await waitFor(() =>
      received.some(
        (value) =>
          (value as { type?: string } | null)?.type === PREVIEW_NET_ERROR,
      ),
    );

    expect(received).toContainEqual(
      expect.objectContaining({
        type: PREVIEW_NET_ERROR,
        detail: expect.stringMatching(/queue limit/),
      }),
    );
    expect(native.write).toHaveBeenCalledTimes(1);
    expect(native.closeFlow).toHaveBeenCalledOnce();
    stop();
  });
});

async function waitFor(predicate: () => boolean): Promise<void> {
  for (let attempt = 0; attempt < 50; attempt++) {
    if (predicate()) return;
    await tick();
  }
  throw new Error("condition was not reached");
}

async function tick(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
}
