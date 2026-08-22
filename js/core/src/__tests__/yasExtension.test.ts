import { describe, expect, it } from "vitest";
import {
  YAS_EXTENSION_LIMIT_MAX_MUTATION_REPLAYS,
  YAS_EXTENSION_MAX_NEXT_START_UNIX_MS,
  YAS_EXTENSION_PHASE_BACKOFF,
  YAS_FAMILY_EXTENSION,
  YAS_FAMILY_LIMIT_POLICIES,
  YAS_GOLDEN_VECTORS,
  YasProtocolError,
  YasWriter,
  decodeExtensionAttemptContext,
  decodeExtensionCommandPage,
  decodeExtensionDeploy,
  decodeExtensionFollowResult,
  decodeExtensionObjectBeginResult,
  decodeExtensionOutputBatch,
  decodeExtensionRecord,
  encodeExtensionAttemptContext,
  encodeExtensionCommandPage,
  encodeExtensionDeploy,
  encodeExtensionFollowResult,
  encodeExtensionObjectBeginResult,
  encodeExtensionOutputBatch,
  encodeExtensionRecord,
  extensionLimitsFromExtensions,
} from "../yas";

function bytes(name: string): Uint8Array {
  const hex = YAS_GOLDEN_VECTORS.vectors.find(
    (entry) => entry.name === name,
  )!.hex;
  return Uint8Array.from(hex.match(/../g)!, (byte) =>
    Number.parseInt(byte, 16),
  );
}

const cases: readonly [string, (payload: Uint8Array) => Uint8Array][] = [
  [
    "extension.object_begin_result.payload",
    (payload) =>
      encodeExtensionObjectBeginResult(
        decodeExtensionObjectBeginResult(payload),
      ),
  ],
  [
    "extension.deploy.payload",
    (payload) => encodeExtensionDeploy(decodeExtensionDeploy(payload)),
  ],
  [
    "extension.state.payload",
    (payload) => encodeExtensionRecord(decodeExtensionRecord(payload)),
  ],
  [
    "extension.follow_result.payload",
    (payload) =>
      encodeExtensionFollowResult(decodeExtensionFollowResult(payload)),
  ],
  [
    "extension.output_batch.payload",
    (payload) =>
      encodeExtensionOutputBatch(decodeExtensionOutputBatch(payload)),
  ],
  [
    "extension.command_page.payload",
    (payload) =>
      encodeExtensionCommandPage(decodeExtensionCommandPage(payload)),
  ],
  [
    "extension.attempt_context.payload",
    (payload) =>
      encodeExtensionAttemptContext(decodeExtensionAttemptContext(payload)),
  ],
];

describe("YAS Extension v1", () => {
  it("round-trips every normative payload and rejects every truncation", () => {
    for (const [name, roundTrip] of cases) {
      const payload = bytes(name);
      expect(roundTrip(payload), name).toEqual(payload);
      for (let end = 0; end < payload.length; end++)
        expect(
          () => roundTrip(payload.subarray(0, end)),
          `${name}@${end}`,
        ).toThrow(YasProtocolError);
    }
  });

  it("rejects output gaps without an exact lost-record count", () => {
    const value = decodeExtensionOutputBatch(
      bytes("extension.output_batch.payload"),
    );
    expect(() =>
      encodeExtensionOutputBatch({
        ...value,
        records: [{ ...value.records[0]!, kind: 3, data: new Uint8Array(7) }],
      }),
    ).toThrow(YasProtocolError);
  });

  it("requires the complete DEPLOY CAS tuple and bounds Unix deadlines", () => {
    const deploy = decodeExtensionDeploy(bytes("extension.deploy.payload"));
    expect(deploy).toMatchObject({
      expectedExtensionHandle: 0n,
      expectedGeneration: 0n,
      expectedDefinitionRevision: 0n,
    });
    expect(() =>
      encodeExtensionDeploy({
        ...deploy,
        expectedExtensionHandle: 1n,
        expectedGeneration: 0n,
        expectedDefinitionRevision: 1n,
      }),
    ).toThrow(YasProtocolError);

    const record = decodeExtensionRecord(bytes("extension.state.payload"));
    expect(() =>
      encodeExtensionRecord({
        ...record,
        phase: YAS_EXTENSION_PHASE_BACKOFF,
        nextStartUnixMs: BigInt(YAS_EXTENSION_MAX_NEXT_START_UNIX_MS) + 1n,
      }),
    ).toThrow(/backoff deadline/);
  });

  it("parses and bounds the advertised mutation replay horizon", () => {
    const limits = YAS_FAMILY_LIMIT_POLICIES[YAS_FAMILY_EXTENSION]!.map(
      ([tag, width, , hardMin]) => ({
        tag,
        required: true,
        value:
          width === 4
            ? new YasWriter().u32(Number(hardMin)).finish()
            : new YasWriter().u64(hardMin).finish(),
      }),
    );
    expect(extensionLimitsFromExtensions(limits).maxMutationReplays).toBe(1);

    const withoutReplayHorizon = limits.filter(
      (limit) => limit.tag !== YAS_EXTENSION_LIMIT_MAX_MUTATION_REPLAYS,
    );
    expect(() => extensionLimitsFromExtensions(withoutReplayHorizon)).toThrow(
      /missing Extension family limit/,
    );
  });
});
