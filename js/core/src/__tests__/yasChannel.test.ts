import { describe, expect, it } from "vitest";
import {
  YAS_CHANNEL_MAX_METADATA_BYTES,
  YAS_GOLDEN_VECTORS,
  decodeChannelAccept,
  decodeChannelConnect,
  decodeChannelListen,
  encodeChannelAccept,
  encodeChannelConnect,
  encodeChannelListen,
} from "../yas";

function fromHex(value: string): Uint8Array {
  return Uint8Array.from(value.match(/../g) ?? [], (byte) =>
    Number.parseInt(byte, 16),
  );
}

function vector(name: string): Uint8Array {
  const value = YAS_GOLDEN_VECTORS.vectors.find(
    (candidate) => candidate.name === name,
  );
  if (!value) throw new Error(`missing generated vector ${name}`);
  return fromHex(value.hex);
}

function everyTruncation<T>(
  bytes: Uint8Array,
  decode: (bytes: Uint8Array) => T,
): T {
  for (let end = 0; end < bytes.length; end++)
    expect(() => decode(bytes.subarray(0, end))).toThrow();
  return decode(bytes);
}

describe("YAS Channel family", () => {
  it("matches the generated LISTEN payload and rejects every truncation", () => {
    const bytes = vector("channel.listen.payload");
    const decoded = everyTruncation(bytes, decodeChannelListen);
    expect(
      encodeChannelListen(
        decoded.name,
        decoded.operationId,
        decoded.metadata,
        decoded.extensions,
      ),
    ).toEqual(bytes);
  });

  it("matches the generated maximum-size LISTEN metadata payload", () => {
    const bytes = vector("channel.listen.max_metadata.payload");
    const decoded = everyTruncation(bytes, decodeChannelListen);
    expect(decoded.metadata).toHaveLength(YAS_CHANNEL_MAX_METADATA_BYTES);
    expect(
      encodeChannelListen(
        decoded.name,
        decoded.operationId,
        decoded.metadata,
        decoded.extensions,
      ),
    ).toEqual(bytes);
  });

  it("matches the generated CONNECT payload and rejects every truncation", () => {
    const bytes = vector("channel.connect.payload");
    const decoded = everyTruncation(bytes, decodeChannelConnect);
    expect(
      encodeChannelConnect(
        decoded.listenerHandle,
        decoded.generation,
        decoded.initialReceiveCredit,
        decoded.metadata,
        decoded.extensions,
      ),
    ).toEqual(bytes);
  });

  it("matches the generated ACCEPT payload and its sensitive MESSAGE transfer", () => {
    const bytes = vector("channel.accept.payload");
    const decoded = everyTruncation(bytes, decodeChannelAccept);
    expect(
      encodeChannelAccept(
        decoded.listenerHandle,
        decoded.generation,
        decoded.endpoint,
      ),
    ).toEqual(bytes);
    expect(decoded.endpoint.descriptor.sensitiveContent).toBe(true);
    expect(decoded.endpoint.descriptor.senderSendCredit).toBe(0n);
  });

  it("rejects zero identities and oversized metadata", () => {
    expect(() => encodeChannelConnect(0n, 1n, 1n, new Uint8Array())).toThrow(
      /handle/,
    );
    expect(() =>
      encodeChannelListen(
        "rpc",
        new Uint8Array(16).fill(1),
        new Uint8Array(YAS_CHANNEL_MAX_METADATA_BYTES + 1),
      ),
    ).toThrow(/metadata/);
  });
});
