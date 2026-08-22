import { describe, expect, it } from "vitest";
import {
  YAS_ENV_MAX_BATCH_BYTES,
  YAS_ENV_SNAPSHOT_CONTENT_KIND,
  YAS_ENV_VERSION,
  YAS_FAMILY_ENV,
  YAS_TRANSFER_MODE_MESSAGE,
  YAS_TRANSFER_SENDER_TO_RECEIVER,
  YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION,
  YasEnvSnapshotAssembler,
  decodeEnvGet,
  decodeEnvGetResult,
  decodeEnvSnapshotBatch,
  encodeEnvGet,
  encodeEnvGetResult,
  encodeEnvSnapshotBatch,
  type YasEnvEntry,
  type YasTransferDescriptor,
} from "../yas";

const entries: readonly YasEnvEntry[] = [
  { key: new TextEncoder().encode("EMPTY"), value: new Uint8Array() },
  {
    key: new TextEncoder().encode("HOME"),
    value: new TextEncoder().encode("/home/example"),
  },
  { key: new Uint8Array([0xff]), value: new Uint8Array([0xfe, 0x3d]) },
];

function descriptor(): YasTransferDescriptor {
  return {
    transferId: 2,
    mode: YAS_TRANSFER_MODE_MESSAGE,
    direction: YAS_TRANSFER_SENDER_TO_RECEIVER,
    flags: 0,
    receiverSendCredit: 0n,
    senderSendCredit: 64n * 1024n,
    maxItemBytes: BigInt(YAS_ENV_MAX_BATCH_BYTES),
    maxChunkBytes: 64 * 1024,
    contentFamily: YAS_FAMILY_ENV,
    contentKind: YAS_ENV_SNAPSHOT_CONTENT_KIND,
    contentVersion: YAS_ENV_VERSION,
    extensions: [
      {
        tag: YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION,
        required: true,
        value: new Uint8Array(),
      },
    ],
    maxOpenMessages: 1,
    sensitiveContent: true,
  };
}

function total(values: readonly YasEnvEntry[]): bigint {
  return values.reduce(
    (sum, entry) => sum + BigInt(entry.key.length + entry.value.length),
    0n,
  );
}

function everyTruncation<T>(
  bytes: Uint8Array,
  decode: (bytes: Uint8Array) => T,
): T {
  for (let end = 0; end < bytes.length; end++)
    expect(() => decode(bytes.subarray(0, end))).toThrow();
  return decode(bytes);
}

describe("YAS Environment family", () => {
  it("round-trips GET and every truncation fails", () => {
    const encoded = encodeEnvGet({ initialReceiveCredit: 4n * 1024n });
    expect(everyTruncation(encoded, decodeEnvGet)).toEqual({
      initialReceiveCredit: 4n * 1024n,
      extensions: [],
    });
  });

  it("round-trips a deterministic raw-byte inline snapshot", () => {
    const encoded = encodeEnvGetResult({
      entryCount: entries.length,
      totalDataBytes: total(entries),
      delivery: { type: "inline", entries },
      extensions: [],
    });
    const decoded = everyTruncation(encoded, decodeEnvGetResult);
    expect(decoded.entryCount).toBe(entries.length);
    expect(decoded.totalDataBytes).toBe(total(entries));
    expect(decoded.delivery.type).toBe("inline");
    if (decoded.delivery.type !== "inline") throw new Error("not inline");
    expect(decoded.delivery.entries.map((entry) => [...entry.key])).toEqual(
      entries.map((entry) => [...entry.key]),
    );
    expect(decoded.delivery.entries.map((entry) => [...entry.value])).toEqual(
      entries.map((entry) => [...entry.value]),
    );
    expect(encodeEnvGetResult(decoded)).toEqual(encoded);
  });

  it("round-trips a sensitive MESSAGE descriptor", () => {
    const encoded = encodeEnvGetResult({
      entryCount: entries.length,
      totalDataBytes: total(entries),
      delivery: { type: "transfer", descriptor: descriptor() },
      extensions: [],
    });
    const decoded = everyTruncation(encoded, decodeEnvGetResult);
    expect(encodeEnvGetResult(decoded)).toEqual(encoded);
    expect(decoded.delivery.type).toBe("transfer");
    if (decoded.delivery.type !== "transfer") throw new Error("not transfer");
    expect(decoded.delivery.descriptor.sensitiveContent).toBe(true);
  });

  it("assembles nonempty contiguous batches", () => {
    const firstBytes = encodeEnvSnapshotBatch({
      firstIndex: 0,
      entries: entries.slice(0, 2),
    });
    const secondBytes = encodeEnvSnapshotBatch({
      firstIndex: 2,
      entries: entries.slice(2),
    });
    const first = everyTruncation(firstBytes, decodeEnvSnapshotBatch);
    const second = everyTruncation(secondBytes, decodeEnvSnapshotBatch);
    const assembler = new YasEnvSnapshotAssembler(
      entries.length,
      total(entries),
    );
    assembler.push(first);
    assembler.push(second);
    const snapshot = assembler.finish();
    expect(snapshot.totalDataBytes).toBe(total(entries));
    expect(snapshot.entries.map((entry) => [...entry.key])).toEqual(
      entries.map((entry) => [...entry.key]),
    );
    expect(snapshot.entries.map((entry) => [...entry.value])).toEqual(
      entries.map((entry) => [...entry.value]),
    );
  });

  it("rejects invalid process entries and ordering", () => {
    expect(() =>
      encodeEnvGetResult({
        entryCount: 1,
        totalDataBytes: 3n,
        delivery: {
          type: "inline",
          entries: [
            { key: new TextEncoder().encode("A=B"), value: new Uint8Array() },
          ],
        },
        extensions: [],
      }),
    ).toThrow(/key/);
    expect(() =>
      encodeEnvGetResult({
        entryCount: 2,
        totalDataBytes: 4n,
        delivery: {
          type: "inline",
          entries: [
            { key: new TextEncoder().encode("B"), value: new Uint8Array([1]) },
            { key: new TextEncoder().encode("A"), value: new Uint8Array([2]) },
          ],
        },
        extensions: [],
      }),
    ).toThrow(/ascending/);
  });
});
