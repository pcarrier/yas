import { describe, expect, it } from "vitest";
import {
  YAS_FS_LIMIT_MAX_CATALOG_ENTRIES,
  YAS_FS_MAX_CATALOG_ENTRIES,
  YAS_FS_RECORD_MOVE,
  YAS_GOLDEN_VECTORS,
  YAS_STATE_ADD,
  YAS_STATE_DELTA,
  YAS_STATE_SNAPSHOT_BEGIN,
  YAS_STATE_SNAPSHOT_END,
  YAS_STATE_SNAPSHOT_RECORDS,
  YasFsCatalog,
  YasProtocolError,
  YasWriter,
  decodeFsApply,
  decodeFsApplyResult,
  decodeFsClose,
  decodeFsCommit,
  decodeFsCommitResult,
  decodeFsConflictDetail,
  decodeFsEntry,
  decodeFsFetch,
  decodeFsGrep,
  decodeFsIndex,
  decodeFsMove,
  decodeFsOpen,
  decodeFsPath,
  decodeFsQueryGrepFileRecord,
  decodeFsQueryGrepMatchRecord,
  decodeFsQueryPage,
  decodeFsQueryPathRecord,
  decodeFsQueryReadRecord,
  decodeFsQueryRecordBatch,
  decodeFsRead,
  decodeFsSearch,
  decodeFsStageWrite,
  decodeFsUnwatch,
  decodeFsWatch,
  encodeFsApply,
  encodeFsApplyResult,
  encodeFsClose,
  encodeFsCommit,
  encodeFsCommitResult,
  encodeFsConflictDetail,
  encodeFsEntry,
  encodeFsFetch,
  encodeFsGrep,
  encodeFsIndex,
  encodeFsMove,
  encodeFsOpen,
  encodeFsPath,
  encodeFsQueryGrepFileRecord,
  encodeFsQueryGrepMatchRecord,
  encodeFsQueryPage,
  encodeFsQueryPathRecord,
  encodeFsQueryReadRecord,
  encodeFsQueryRecordBatch,
  encodeFsRead,
  encodeFsSearch,
  encodeFsStageWrite,
  encodeFsUnwatch,
  encodeFsWatch,
  type YasFsSnapshot,
  type YasStateBatch,
} from "../yas";

function bytes(name: string): Uint8Array {
  const hex = YAS_GOLDEN_VECTORS.vectors.find(
    (entry) => entry.name === name,
  )!.hex;
  return Uint8Array.from(hex.match(/../g)!, (byte) =>
    Number.parseInt(byte, 16),
  );
}

function catalogConnection(): never {
  return {
    onInvalidation: () => () => undefined,
    family: () => ({
      limits: [
        {
          tag: YAS_FS_LIMIT_MAX_CATALOG_ENTRIES,
          value: new YasWriter().u32(YAS_FS_MAX_CATALOG_ENTRIES).finish(),
        },
      ],
    }),
  } as never;
}

const cases: readonly [string, (payload: Uint8Array) => Uint8Array][] = [
  ["fs.open.payload", (payload) => encodeFsOpen(decodeFsOpen(payload))],
  [
    "fs.close.payload",
    (payload) => {
      const value = decodeFsClose(payload);
      return encodeFsClose(value.rootHandle, value.extensions);
    },
  ],
  [
    "fs.watch.payload",
    (payload) => {
      const value = decodeFsWatch(payload);
      return encodeFsWatch(
        value.rootHandle,
        value.flags,
        value.settleMs,
        value.inlineMax,
        value.ignorePatterns,
        value.encodedStateWatch,
      );
    },
  ],
  [
    "fs.unwatch.payload",
    (payload) => encodeFsUnwatch(decodeFsUnwatch(payload)),
  ],
  ["fs.fetch.payload", (payload) => encodeFsFetch(decodeFsFetch(payload))],
  ["fs.read.payload", (payload) => encodeFsRead(decodeFsRead(payload))],
  ["fs.search.payload", (payload) => encodeFsSearch(decodeFsSearch(payload))],
  ["fs.index.payload", (payload) => encodeFsIndex(decodeFsIndex(payload))],
  ["fs.grep.payload", (payload) => encodeFsGrep(decodeFsGrep(payload))],
  [
    "fs.stage_write.payload",
    (payload) => encodeFsStageWrite(decodeFsStageWrite(payload)),
  ],
  ["fs.commit.payload", (payload) => encodeFsCommit(decodeFsCommit(payload))],
  [
    "fs.commit_result.payload",
    (payload) => encodeFsCommitResult(decodeFsCommitResult(payload)),
  ],
  [
    "fs.conflict_detail.payload",
    (payload) => encodeFsConflictDetail(decodeFsConflictDetail(payload)),
  ],
  ["fs.apply.payload", (payload) => encodeFsApply(decodeFsApply(payload))],
  [
    "fs.apply_result.payload",
    (payload) => encodeFsApplyResult(decodeFsApplyResult(payload)),
  ],
  [
    "fs.entry.inline.payload",
    (payload) => encodeFsEntry(decodeFsEntry(payload)),
  ],
  [
    "fs.query.inline.payload",
    (payload) => encodeFsQueryPage(decodeFsQueryPage(payload)),
  ],
  [
    "fs.query.batch.payload",
    (payload) => encodeFsQueryRecordBatch(decodeFsQueryRecordBatch(payload)),
  ],
  [
    "fs.query.read_record.payload",
    (payload) => encodeFsQueryReadRecord(decodeFsQueryReadRecord(payload)),
  ],
  [
    "fs.query.path_record.payload",
    (payload) => encodeFsQueryPathRecord(decodeFsQueryPathRecord(payload)),
  ],
  [
    "fs.query.grep_file_record.payload",
    (payload) =>
      encodeFsQueryGrepFileRecord(decodeFsQueryGrepFileRecord(payload)),
  ],
  [
    "fs.query.grep_match_record.payload",
    (payload) =>
      encodeFsQueryGrepMatchRecord(decodeFsQueryGrepMatchRecord(payload)),
  ],
  ["fs.state.move.payload", (payload) => encodeFsMove(decodeFsMove(payload))],
];

describe("YAS FS v1", () => {
  it("round-trips every normative payload and rejects every truncation", () => {
    for (const [name, roundTrip] of cases) {
      const payload = bytes(name);
      expect(roundTrip(payload)).toEqual(payload);
      for (let end = 0; end < payload.length; end++)
        expect(() => roundTrip(payload.subarray(0, end))).toThrow(
          YasProtocolError,
        );
    }
  });

  it("preserves raw path bytes and rejects traversal components", () => {
    const path = {
      components: [new Uint8Array([0xff, 0x61]), new Uint8Array([0x62])],
    };
    expect(decodeFsPath(encodeFsPath(path))).toEqual(path);
    for (const component of [
      new Uint8Array(),
      new Uint8Array([0x2e]),
      new Uint8Array([0x2e, 0x2e]),
      new Uint8Array([0x61, 0x2f, 0x62]),
      new Uint8Array([0x61, 0x5c, 0x62]),
      new Uint8Array([0x61, 0, 0x62]),
    ])
      expect(() => encodeFsPath({ components: [component] })).toThrow(
        YasProtocolError,
      );
  });

  it("applies snapshot and first-class MOVE records without losing metadata", () => {
    const connection = catalogConnection();
    const catalog = new YasFsCatalog(connection, 1n);
    const apply = (
      catalog as unknown as { apply(batch: YasStateBatch): void }
    ).apply.bind(catalog);
    let latest: YasFsSnapshot | undefined;
    catalog.subscribe((snapshot) => {
      latest = snapshot;
    });
    const entry = decodeFsEntry(bytes("fs.entry.inline.payload"));
    apply(batch(YAS_STATE_SNAPSHOT_BEGIN, 0n, 1n));
    apply(
      batch(YAS_STATE_SNAPSHOT_RECORDS, 1n, 1n, [
        { kind: YAS_STATE_ADD, flags: 0, body: encodeFsEntry(entry) },
      ]),
    );
    apply(batch(YAS_STATE_SNAPSHOT_END, 1n, 1n));
    expect(latest?.entries).toHaveLength(1);

    const move = decodeFsMove(bytes("fs.state.move.payload"));
    apply(
      batch(YAS_STATE_DELTA, 1n, 2n, [
        { kind: YAS_FS_RECORD_MOVE, flags: 0, body: encodeFsMove(move) },
      ]),
    );
    expect(latest?.revision).toBe(2n);
    expect(latest?.entries[0]).toMatchObject({
      entryRevision: entry.entryRevision,
      mode: entry.mode,
      path: move.to,
    });
  });

  it("exposes validated state batches without reconstructing phase boundaries", () => {
    const connection = catalogConnection();
    const catalog = new YasFsCatalog(connection, 1n);
    const apply = (
      catalog as unknown as { apply(batch: YasStateBatch): void }
    ).apply.bind(catalog);
    const phases: number[] = [];
    const remove = catalog.subscribeBatches((state) => {
      phases.push(state.phase);
    });

    apply(batch(YAS_STATE_SNAPSHOT_BEGIN, 0n, 1n));
    apply(batch(YAS_STATE_SNAPSHOT_RECORDS, 1n, 1n));
    apply(batch(YAS_STATE_SNAPSHOT_END, 1n, 1n));
    remove();
    apply(batch(YAS_STATE_DELTA, 1n, 2n));

    expect(phases).toEqual([
      YAS_STATE_SNAPSHOT_BEGIN,
      YAS_STATE_SNAPSHOT_RECORDS,
      YAS_STATE_SNAPSHOT_END,
    ]);
  });
});

function batch(
  phase: number,
  fromRevision: bigint,
  toRevision: bigint,
  records: YasStateBatch["records"] = [],
): YasStateBatch {
  return { phase, flags: 0, fromRevision, toRevision, records };
}
