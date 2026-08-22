import { describe, expect, it, vi } from "vitest";
import {
  YAS_GOLDEN_VECTORS,
  YAS_NET_DELIVERY_RELIABLE_TUNNEL,
  YAS_NET_DIRECTION_DUPLEX,
  YAS_NET_MODE_DATAGRAM,
  YasNetFlow,
  YasProtocolError,
  decodeNetDatagram,
  decodeNetDatagramStats,
  decodeNetEndpoint,
  decodeNetOpen,
  encodeNetDatagram,
  encodeNetDatagramStats,
  encodeNetEndpoint,
  encodeNetOpen,
} from "../yas";

function bytes(name: string): Uint8Array {
  const hex = YAS_GOLDEN_VECTORS.vectors.find(
    (entry) => entry.name === name,
  )!.hex;
  return Uint8Array.from(
    hex.match(/../g)!.map((byte) => Number.parseInt(byte, 16)),
  );
}

describe("YAS Net v1", () => {
  it("round-trips the four normative payloads", () => {
    const open = decodeNetOpen(bytes("net.open.payload"));
    expect(encodeNetOpen(open)).toEqual(bytes("net.open.payload"));
    const endpoint = decodeNetEndpoint(bytes("net.endpoint.payload"));
    expect(encodeNetEndpoint(endpoint)).toEqual(bytes("net.endpoint.payload"));
    const datagram = decodeNetDatagram(bytes("net.datagram.payload"));
    expect(encodeNetDatagram(datagram)).toEqual(bytes("net.datagram.payload"));
    const stats = decodeNetDatagramStats(bytes("net.datagram_stats.payload"));
    expect(encodeNetDatagramStats(stats)).toEqual(
      bytes("net.datagram_stats.payload"),
    );
  });

  it("rejects fixed-body truncations and invalid cross-mode options", () => {
    for (const [name, decode] of [
      ["net.open.payload", decodeNetOpen],
      ["net.endpoint.payload", decodeNetEndpoint],
      ["net.datagram_stats.payload", decodeNetDatagramStats],
    ] as const) {
      const payload = bytes(name);
      for (let end = 0; end < payload.length; end++)
        expect(() => decode(payload.subarray(0, end))).toThrow(
          YasProtocolError,
        );
    }
    const open = decodeNetOpen(bytes("net.open.payload"));
    expect(() =>
      encodeNetOpen({
        ...open,
        address: { kind: "udp", host: "localhost", port: 53 },
      }),
    ).toThrow(/datagram/);
  });

  it("surfaces gaps while preserving reordered and duplicate datagrams", () => {
    const sendEvent = vi.fn();
    const flow = new YasNetFlow({ connection: { sendEvent } } as never, {
      flowHandle: 9n,
      mode: YAS_NET_MODE_DATAGRAM,
      direction: YAS_NET_DIRECTION_DUPLEX,
      selectedDelivery: YAS_NET_DELIVERY_RELIABLE_TUNNEL,
      maxDatagramPayload: 1200,
      serverInstanceLimit: 0,
      maxMessageBytes: 0n,
      peerAddress: { kind: "udp", host: "127.0.0.1", port: 53 },
      negotiatedAlpn: new Uint8Array(0),
      extensions: [],
    });
    const received: Array<[bigint, bigint]> = [];
    flow.onDatagram((value) =>
      received.push([value.sequence, value.droppedBefore]),
    );
    flow.receiveDatagram({
      flowHandle: 9n,
      sequence: 3n,
      payload: new Uint8Array(1),
    });
    flow.receiveDatagram({
      flowHandle: 9n,
      sequence: 7n,
      payload: new Uint8Array(1),
    });
    flow.receiveDatagram({
      flowHandle: 9n,
      sequence: 6n,
      payload: new Uint8Array(1),
    });
    flow.receiveDatagram({
      flowHandle: 9n,
      sequence: 7n,
      payload: new Uint8Array(1),
    });
    expect(received).toEqual([
      [3n, 3n],
      [7n, 3n],
      [6n, 0n],
      [7n, 0n],
    ]);
    expect(flow.sendDatagram(new Uint8Array([1]))).toBe(0n);
    expect(sendEvent).toHaveBeenCalledOnce();
  });
});
