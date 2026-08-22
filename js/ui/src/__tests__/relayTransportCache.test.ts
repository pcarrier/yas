import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  YasTransport,
  YasTransportMessage,
  ConnectionStatus,
  YasRelayRoute,
  YasWorkspaceConnection,
} from "@yas-run/core";
import {
  boundedRelayRoutes,
  RelayConnectionCache,
  UI_RELAY_MAX_DESCRIPTION_CHARS,
  UI_RELAY_MAX_LABEL_CHARS,
  UI_RELAY_MAX_NAME_CHARS,
  UI_RELAY_MAX_ROUTES,
} from "../relayTransportCache";

class TestTransport implements YasTransport {
  status: ConnectionStatus = "disconnected";
  readonly authRejected = false;
  readonly maxDatagramSize = 0;
  lastError: string | null = null;
  close = vi.fn(() => {
    this.status = "closed";
    for (const listener of this.statusListeners) listener("closed");
  });
  private readonly messageListeners = new Set<
    (message: YasTransportMessage) => void
  >();
  private readonly statusListeners = new Set<
    (status: ConnectionStatus) => void
  >();

  connect(): void {}
  reconnect(): void {}
  suspend(): void {}
  send(): void {}

  addEventListener(
    type: "message" | "datagram",
    listener: (message: YasTransportMessage) => void,
  ): void;
  addEventListener(
    type: "statuschange",
    listener: (status: ConnectionStatus) => void,
  ): void;
  addEventListener(
    type: "message" | "datagram" | "statuschange",
    listener:
      | ((message: YasTransportMessage) => void)
      | ((status: ConnectionStatus) => void),
  ): void {
    if (type === "message") {
      this.messageListeners.add(
        listener as (message: YasTransportMessage) => void,
      );
    } else if (type === "statuschange") {
      this.statusListeners.add(listener as (status: ConnectionStatus) => void);
    }
  }

  removeEventListener(
    type: "message" | "datagram",
    listener: (message: YasTransportMessage) => void,
  ): void;
  removeEventListener(
    type: "statuschange",
    listener: (status: ConnectionStatus) => void,
  ): void;
  removeEventListener(
    type: "message" | "datagram" | "statuschange",
    listener:
      | ((message: YasTransportMessage) => void)
      | ((status: ConnectionStatus) => void),
  ): void {
    if (type === "message") {
      this.messageListeners.delete(
        listener as (message: YasTransportMessage) => void,
      );
    } else if (type === "statuschange") {
      this.statusListeners.delete(
        listener as (status: ConnectionStatus) => void,
      );
    }
  }

  emit(status: ConnectionStatus): void {
    this.status = status;
    for (const listener of this.statusListeners) listener(status);
  }
}

function connection(transport: TestTransport): YasWorkspaceConnection {
  return {
    transport,
    close: () => transport.close(),
    dispose: vi.fn(),
  } as unknown as YasWorkspaceConnection;
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe("RelayConnectionCache", () => {
  it("invalidates a permanently closed connection and backs off replacements", async () => {
    const retry = vi.fn();
    const cache = new RelayConnectionCache(retry, 500, 10_000);
    const first = new TestTransport();
    cache.set("work", "1:1", connection(first));

    first.emit("error");
    await vi.advanceTimersByTimeAsync(499);
    expect(retry).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(retry).toHaveBeenCalledTimes(1);
    expect(cache.get("work")).toBeUndefined();
    expect(first.close).toHaveBeenCalledOnce();

    const second = new TestTransport();
    cache.set("work", "1:1", connection(second));
    second.emit("closed");
    await vi.advanceTimersByTimeAsync(999);
    expect(retry).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(retry).toHaveBeenCalledTimes(2);
  });

  it("cancels retry ownership when a route is removed", async () => {
    const retry = vi.fn();
    const cache = new RelayConnectionCache(retry, 500, 10_000);
    const transport = new TestTransport();
    cache.set("work", "1:1", connection(transport));
    transport.emit("error");

    cache.delete("work");
    await vi.advanceTimersByTimeAsync(10_000);

    expect(retry).not.toHaveBeenCalled();
    expect(transport.close).toHaveBeenCalledOnce();
  });

  it("resets retry delay after a replacement connects", async () => {
    const retry = vi.fn();
    const cache = new RelayConnectionCache(retry, 500, 10_000);
    const first = new TestTransport();
    cache.set("work", "1:1", connection(first));
    first.emit("error");
    await vi.advanceTimersByTimeAsync(500);

    const second = new TestTransport();
    cache.set("work", "1:1", connection(second));
    second.emit("connected");
    second.emit("error");
    await vi.advanceTimersByTimeAsync(499);
    expect(retry).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(retry).toHaveBeenCalledTimes(2);
  });

  it("drops retry-only tombstones when route names rotate", async () => {
    const cache = new RelayConnectionCache(() => {}, 1, 10);
    for (let i = 0; i < 100; i++) {
      const transport = new TestTransport();
      cache.set(`route-${i}`, "1:1", connection(transport));
      transport.emit("error");
      await vi.advanceTimersByTimeAsync(1);
    }
    expect(cache.stats()).toEqual({ entries: 0, retryDelays: 100 });
    cache.retain(new Set());
    expect(cache.stats()).toEqual({ entries: 0, retryDelays: 0 });
  });
});

describe("boundedRelayRoutes", () => {
  const route = (name: string): YasRelayRoute => ({
    handle: 1n,
    generation: 1n,
    availability: 0,
    transportHint: 0,
    flags: 0,
    name,
    label: "l".repeat(UI_RELAY_MAX_LABEL_CHARS + 10),
    description: "d".repeat(UI_RELAY_MAX_DESCRIPTION_CHARS + 10),
    extensions: [],
  });

  it("bounds route rotation, rejects oversized names, and trims presentation", () => {
    const routes = [
      route("x".repeat(UI_RELAY_MAX_NAME_CHARS + 1)),
      ...Array.from({ length: UI_RELAY_MAX_ROUTES + 20 }, (_, i) =>
        route(`route-${i}`),
      ),
    ];
    const bounded = boundedRelayRoutes(routes);
    expect(bounded).toHaveLength(UI_RELAY_MAX_ROUTES);
    expect(bounded[0].name).toBe("route-0");
    expect(bounded[0].label).toHaveLength(UI_RELAY_MAX_LABEL_CHARS);
    expect(bounded[0].description).toHaveLength(UI_RELAY_MAX_DESCRIPTION_CHARS);
  });
});
