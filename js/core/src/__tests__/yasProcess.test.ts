import { describe, expect, it } from "vitest";
import {
  YAS_GOLDEN_VECTORS,
  YAS_PROCESS_CONTROL_SIGNAL,
  YAS_PROCESS_ENV_EMPTY,
  YAS_PROCESS_SIGNAL_INTERRUPT,
  YasProtocolError,
  decodeProcessExit,
  decodeProcessSpawn,
  decodeProcessStreamBundle,
  encodeProcessControl,
  encodeProcessExit,
  encodeProcessSpawn,
  encodeProcessStreamBundle,
} from "../yas";

function bytes(name: string): Uint8Array {
  const hex = YAS_GOLDEN_VECTORS.vectors.find(
    (entry) => entry.name === name,
  )!.hex;
  return Uint8Array.from(
    hex.match(/../g)!.map((byte) => Number.parseInt(byte, 16)),
  );
}

describe("YAS Process v1", () => {
  it("round-trips every normative payload and rejects every truncation", () => {
    const cases = [
      ["process.spawn.payload", decodeProcessSpawn, encodeProcessSpawn],
      [
        "process.stream_bundle.payload",
        decodeProcessStreamBundle,
        encodeProcessStreamBundle,
      ],
      ["process.exit.payload", decodeProcessExit, encodeProcessExit],
    ] as const;
    for (const [name, decode, encode] of cases) {
      const payload = bytes(name);
      const value = decode(payload as never);
      expect(encode(value as never)).toEqual(payload);
      for (let end = 0; end < payload.length; end++)
        expect(() => decode(payload.subarray(0, end) as never)).toThrow(
          YasProtocolError,
        );
    }
  });

  it("preserves raw argv and enforces sorted environment and control values", () => {
    const value = decodeProcessSpawn(bytes("process.spawn.payload"));
    expect(value.argv[1]).toEqual(new Uint8Array([0xff, 0x61, 0x72, 0x67]));
    expect(() =>
      encodeProcessSpawn({
        ...value,
        environmentKind: YAS_PROCESS_ENV_EMPTY,
        environment: [
          { key: new Uint8Array([0x42]), value: new Uint8Array(0) },
          { key: new Uint8Array([0x41]), value: new Uint8Array(0) },
        ],
      }),
    ).toThrow(/environment/);
    expect(() =>
      encodeProcessControl({
        processHandle: 1n,
        operationId: new Uint8Array(16).fill(1),
        action: YAS_PROCESS_CONTROL_SIGNAL,
        value: YAS_PROCESS_SIGNAL_INTERRUPT - 1,
      }),
    ).toThrow(/signal/);
  });

  it("retains lifetime offsets and merged-stderr shape", () => {
    const bundle = decodeProcessStreamBundle(
      bytes("process.stream_bundle.payload"),
    );
    expect(bundle.stdoutLifetimeOffset).toBe(10n);
    expect(bundle.stderrLifetimeOffset).toBe(20n);
    expect(bundle.mergedStderr).toBe(false);
    expect(bundle.stdin).toBeDefined();
    expect(bundle.stderr).toBeDefined();
  });
});
