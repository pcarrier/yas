import { describe, expect, it } from "vitest";
import {
  YAS_TERMINAL_GOLDEN_FRAME_FLAGS,
  YasProtocolError,
  YasWriter,
  decodeTerminalGridV1,
  decodeYasTerminalRecording,
} from "../yas";

const packedGrid = hex("01000100000000000000000000000000000000000000000000");

function hex(value: string): Uint8Array {
  return new Uint8Array(
    value.match(/../g)!.map((byte) => Number.parseInt(byte, 16)),
  );
}

function recording(frameSequence = 42, timestamp = 123_456n): Uint8Array {
  const frame = new YasWriter()
    .u32(7)
    .u32(frameSequence)
    .u16(YAS_TERMINAL_GOLDEN_FRAME_FLAGS)
    .bytes(packedGrid)
    .finish();
  return new YasWriter()
    .bytes(new TextEncoder().encode("YASREC1\n"))
    .u32(36)
    .u16(0)
    .u16(1)
    .u64(9n)
    .u32(3)
    .u16(1)
    .u16(1)
    .u32(7)
    .u32(42)
    .u64(1_000_000n)
    .u64(timestamp)
    .u32(frame.length)
    .bytes(frame)
    .finish();
}

describe("YASREC1", () => {
  it("decodes native TerminalFrame recordings and their first grid", () => {
    const value = decodeYasTerminalRecording(recording());
    expect(value.header).toMatchObject({
      gridCodec: 1,
      terminalHandle: 9n,
      generation: 3,
      rows: 1,
      cols: 1,
      viewId: 7,
      firstSequence: 42,
      ticksPerSecond: 1_000_000n,
    });
    expect(value.frames).toHaveLength(1);
    expect(value.frames[0]!.timestampTicks).toBe(123_456n);
    const grid = decodeTerminalGridV1(value.frames[0]!.frame, null, 1024);
    expect(grid).toMatchObject({ sequence: 42, rows: 1, cols: 1 });
  });

  it("rejects invalid-magic, truncated, and sequence-invalid recordings", () => {
    expect(() =>
      decodeYasTerminalRecording(new TextEncoder().encode("NOTYAS!\n")),
    ).toThrow(/magic/);
    expect(() =>
      decodeYasTerminalRecording(recording().subarray(0, -1)),
    ).toThrow(YasProtocolError);
    expect(() => decodeYasTerminalRecording(recording(43))).toThrow(
      /sequence 43, expected 42/,
    );
  });

  it("enforces caller-supplied byte and frame limits", () => {
    const bytes = recording();
    expect(() =>
      decodeYasTerminalRecording(bytes, { maxBytes: bytes.length - 1 }),
    ).toThrow(/byte limit/);
    expect(() =>
      decodeYasTerminalRecording(bytes, { maxFrames: 1, maxFrameBytes: 1 }),
    ).toThrow(/TerminalFrame length/);
  });
});
