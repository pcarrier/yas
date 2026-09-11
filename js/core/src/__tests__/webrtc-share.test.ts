// @vitest-environment node
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import nacl from "tweetnacl";
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

  setRemoteDescription = vi.fn(
    async (_description: RTCSessionDescriptionInit) => {},
  );
  addIceCandidate = vi.fn(async (_candidate: RTCIceCandidateInit) => {});

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

const OLD_PRODUCER = "00000000-0000-4000-8000-000000000001";
const NEW_PRODUCER = "00000000-0000-4000-8000-000000000002";

function join(socket: MockWebSocket, sessionId: string, role = "producer") {
  socket.receive({ type: "peer_joined", role, sessionId });
}

function signal(socket: MockWebSocket, from: string, data: unknown) {
  // The fake PBKDF2 above derives zero-filled keys for both peers.
  const secret = new Uint8Array(32);
  const nonce = new Uint8Array(nacl.box.nonceLength);
  const encrypted = nacl.box(
    new TextEncoder().encode(JSON.stringify(data)),
    nonce,
    nacl.scalarMult.base(secret),
    secret,
  );
  const box = btoa(String.fromCharCode(...nonce, ...encrypted));
  socket.receive({ type: "signal", from, data: { box } });
}

describe("createShareTransport", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    MockPeerConnection.instances = [];
    MockWebSocket.instances = [];
    vi.stubGlobal("RTCPeerConnection", MockPeerConnection);
    vi.stubGlobal("WebSocket", MockWebSocket);
    vi.stubGlobal(
      "RTCSessionDescription",
      class {
        constructor(description: RTCSessionDescriptionInit) {
          Object.assign(this, description);
        }
      },
    );
    vi.stubGlobal(
      "RTCIceCandidate",
      class {
        constructor(candidate: RTCIceCandidateInit) {
          Object.assign(this, candidate);
        }
      },
    );
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

  it("offers to every producer advertised while the SDP is being created", async () => {
    const transport = createShareTransport("wss://hub.example", "secret");
    transport.connect();
    await settle();
    const socket = MockWebSocket.instances[0]!;
    socket.receive({ type: "registered", sessionId: "consumer" });
    join(socket, OLD_PRODUCER);
    join(socket, NEW_PRODUCER);
    join(socket, "another-consumer", "consumer");
    await settle();

    expect(socket.sent.map((message) => JSON.parse(message).target)).toEqual([
      OLD_PRODUCER,
      NEW_PRODUCER,
    ]);
    transport.close();
  });

  it("replays the offer and gathered ICE to a producer that joins later", async () => {
    const transport = createShareTransport("wss://hub.example", "secret");
    transport.connect();
    await settle();
    const socket = MockWebSocket.instances[0]!;
    socket.receive({ type: "registered", sessionId: "consumer" });
    join(socket, OLD_PRODUCER);
    await settle();
    const pc = MockPeerConnection.instances[0]!;
    const candidate = { candidate: "candidate:local", sdpMid: "0" };
    pc.onicecandidate?.({
      candidate: { ...candidate, toJSON: () => candidate },
    } as RTCPeerConnectionIceEvent);
    join(socket, NEW_PRODUCER);
    join(socket, NEW_PRODUCER);
    join(socket, "another-consumer", "consumer");

    expect(socket.sent.map((message) => JSON.parse(message).target)).toEqual([
      OLD_PRODUCER,
      OLD_PRODUCER,
      NEW_PRODUCER,
      NEW_PRODUCER,
    ]);
    expect(MockPeerConnection.instances).toHaveLength(1);
    transport.close();
  });

  it("uses the first authenticated answer and only that producer's ICE", async () => {
    const transport = createShareTransport("wss://hub.example", "secret");
    transport.connect();
    await settle();
    const socket = MockWebSocket.instances[0]!;
    socket.receive({ type: "registered", sessionId: "consumer" });
    join(socket, OLD_PRODUCER);
    join(socket, NEW_PRODUCER);
    await settle();
    const pc = MockPeerConnection.instances[0]!;
    signal(socket, OLD_PRODUCER, {
      candidate: { candidate: "candidate:stale" },
    });
    signal(socket, NEW_PRODUCER, {
      candidate: { candidate: "candidate:live" },
    });
    const answer = { type: "answer", sdp: "v=0\r\n" };
    signal(socket, "unknown-producer", { sdp: answer });
    expect(pc.setRemoteDescription).not.toHaveBeenCalled();
    signal(socket, NEW_PRODUCER, { sdp: answer });
    signal(socket, OLD_PRODUCER, { sdp: answer });
    await settle();

    expect(pc.setRemoteDescription).toHaveBeenCalledTimes(1);
    expect(pc.addIceCandidate).toHaveBeenCalledExactlyOnceWith(
      expect.objectContaining({ candidate: "candidate:live" }),
    );
    signal(socket, OLD_PRODUCER, {
      candidate: { candidate: "candidate:late-stale" },
    });
    expect(pc.addIceCandidate).toHaveBeenCalledTimes(1);
    const candidate = { candidate: "candidate:local" };
    pc.onicecandidate?.({
      candidate: { ...candidate, toJSON: () => candidate },
    } as RTCPeerConnectionIceEvent);
    expect(JSON.parse(socket.sent.at(-1)!).target).toBe(NEW_PRODUCER);
    transport.close();
  });
});
