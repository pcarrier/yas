import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createShareTransport } from "../transports/webrtc-share";

class MockDataChannel {
  binaryType = "arraybuffer";
  readyState: RTCDataChannelState = "connecting";
  bufferedAmount = 0;
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  onclose: ((event: Event) => void) | null = null;

  close(): void {
    this.readyState = "closed";
  }

  send(): void {}
}

class MockPeerConnection {
  static instances: MockPeerConnection[] = [];

  connectionState: RTCPeerConnectionState = "new";
  iceConnectionState: RTCIceConnectionState = "new";
  iceGatheringState: RTCIceGatheringState = "new";
  signalingState: RTCSignalingState = "stable";
  onconnectionstatechange: (() => void) | null = null;
  oniceconnectionstatechange: (() => void) | null = null;
  onicegatheringstatechange: (() => void) | null = null;
  onsignalingstatechange: (() => void) | null = null;
  onicecandidate: ((event: RTCPeerConnectionIceEvent) => void) | null = null;
  closed = false;
  private readonly listeners = new Map<string, Set<() => void>>();

  constructor() {
    MockPeerConnection.instances.push(this);
  }

  createDataChannel(): RTCDataChannel {
    return new MockDataChannel() as unknown as RTCDataChannel;
  }

  async createOffer(): Promise<RTCSessionDescriptionInit> {
    return { type: "offer", sdp: "v=0\r\n" };
  }

  async setLocalDescription(): Promise<void> {
    this.signalingState = "have-local-offer";
  }

  addEventListener(type: string, listener: () => void): void {
    let listeners = this.listeners.get(type);
    if (!listeners) {
      listeners = new Set();
      this.listeners.set(type, listeners);
    }
    listeners.add(listener);
  }

  removeEventListener(type: string, listener: () => void): void {
    this.listeners.get(type)?.delete(listener);
  }

  close(): void {
    this.closed = true;
    this.connectionState = "closed";
  }
}

class MockWebSocket {
  static instances: MockWebSocket[] = [];

  onopen: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onmessage: ((event: MessageEvent<string>) => void) | null = null;
  onclose: (() => void) | null = null;
  readonly sent: string[] = [];
  closed = false;

  constructor(readonly url: string) {
    MockWebSocket.instances.push(this);
    queueMicrotask(() => this.onopen?.());
  }

  send(message: string): void {
    this.sent.push(message);
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    this.onclose?.();
  }

  receive(message: unknown): void {
    this.onmessage?.(
      new MessageEvent("message", { data: JSON.stringify(message) }),
    );
  }
}

async function settle(rounds = 20): Promise<void> {
  for (let round = 0; round < rounds; round++) await Promise.resolve();
}

describe("createShareTransport", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    MockPeerConnection.instances = [];
    MockWebSocket.instances = [];
    vi.stubGlobal("RTCPeerConnection", MockPeerConnection);
    vi.stubGlobal("WebSocket", MockWebSocket);
    vi.stubGlobal("fetch", async () => ({
      json: async () => ({ iceServers: [] }),
    }));
    vi.stubGlobal("crypto", {
      getRandomValues(bytes: Uint8Array) {
        bytes.fill(7);
        return bytes;
      },
      subtle: {
        async importKey() {
          return {};
        },
        async deriveBits() {
          return new Uint8Array(32).buffer;
        },
      },
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("does not cut restrictive-network ICE off after ten seconds", async () => {
    const transport = createShareTransport("wss://hub.example", "secret");
    transport.connect();
    await settle();

    const signaling = MockWebSocket.instances[0]!;
    signaling.receive({ type: "registered", sessionId: "consumer" });
    signaling.receive({
      type: "peer_joined",
      role: "producer",
      sessionId: "00000000-0000-4000-8000-000000000001",
    });
    await settle();
    expect(MockPeerConnection.instances).toHaveLength(1);

    vi.advanceTimersByTime(10_000);
    expect(transport.status).toBe("connecting");
    transport.close();
  });

  it("does not arm a stale delayed retry behind a synchronous reconnect", async () => {
    const transport = createShareTransport("wss://hub.example", "secret");
    transport.addEventListener("statuschange", ((status: string) => {
      if (status === "error") transport.connect();
    }) as never);
    transport.connect();
    await settle();

    const firstSignaling = MockWebSocket.instances[0]!;
    firstSignaling.receive({ type: "registered", sessionId: "consumer" });
    firstSignaling.receive({
      type: "peer_joined",
      role: "producer",
      sessionId: "00000000-0000-4000-8000-000000000001",
    });
    await settle();

    vi.advanceTimersByTime(30_000);
    await settle();
    expect(MockWebSocket.instances).toHaveLength(2);
    const freshSignaling = MockWebSocket.instances[1]!;
    expect(firstSignaling.closed).toBe(true);
    expect(freshSignaling.closed).toBe(false);

    vi.advanceTimersByTime(1_000);
    await settle();
    expect(MockWebSocket.instances).toHaveLength(2);
    expect(freshSignaling.closed).toBe(false);
    transport.close();
  });
});
