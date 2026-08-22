import { describe, expect, it } from "vitest";
import {
  YAS_GOLDEN_VECTORS,
  YAS_KV_MAX_INLINE_BYTES,
  decodeKvBatch,
  decodeKvEntry,
  decodeKvGetResult,
  decodeKvMutationResult,
  decodeKvOpen,
  decodeKvPut,
  decodeKvStageValueResult,
  decodeKvWatch,
  encodeKvBatch,
  encodeKvEntry,
  encodeKvGetResult,
  encodeKvMutationResult,
  encodeKvOpen,
  encodeKvPut,
  encodeKvStageValueResult,
  encodeKvWatch,
  validateKvFullKey,
} from "../yas";

const cases = [
  ["kv.open.payload", decodeKvOpen, encodeKvOpen],
  ["kv.watch.payload", decodeKvWatch, encodeKvWatch],
  ["kv.entry.inline.payload", decodeKvEntry, encodeKvEntry],
  ["kv.get.transfer.payload", decodeKvGetResult, encodeKvGetResult],
  [
    "kv.stage_value.result.payload",
    decodeKvStageValueResult,
    encodeKvStageValueResult,
  ],
  ["kv.put.inline.payload", decodeKvPut, encodeKvPut],
  [
    "kv.mutation_result.payload",
    decodeKvMutationResult,
    encodeKvMutationResult,
  ],
  ["kv.batch.payload", decodeKvBatch, encodeKvBatch],
] as const;

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

describe("YAS KV family", () => {
  for (const [name, decode, encode] of cases) {
    it(`${name} matches Rust and rejects every truncation`, () => {
      const bytes = vector(name);
      for (let end = 0; end < bytes.length; end++)
        expect(() => decode(bytes.subarray(0, end) as never)).toThrow();
      const value = decode(bytes as never);
      expect(encode(value as never)).toEqual(bytes);
    });
  }

  it("keeps keys raw and validates the combined namespace key", () => {
    expect(() =>
      validateKvFullKey(new Uint8Array(), new Uint8Array()),
    ).toThrow();
    expect(() =>
      validateKvFullKey(new Uint8Array([1]), new Uint8Array(256).fill(2)),
    ).toThrow();
    expect(() =>
      encodeKvEntry({
        relativeKey: new Uint8Array([0xff]),
        contentHash: new Uint8Array(32),
        byteLength: BigInt(YAS_KV_MAX_INLINE_BYTES + 1),
        modificationRevision: 1n,
        modifiedUnixNs: 1n,
        inlineValue: new Uint8Array(YAS_KV_MAX_INLINE_BYTES + 1),
        extensions: [],
      }),
    ).toThrow(/inline/);
  });
});
