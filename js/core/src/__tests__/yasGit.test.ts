import { describe, expect, it } from "vitest";
import {
  YAS_GOLDEN_VECTORS,
  YasProtocolError,
  decodeGitClose,
  decodeGitClosed,
  decodeGitBlameRecord,
  decodeGitCommitRecord,
  decodeGitContentRecord,
  decodeGitDiffRecord,
  decodeGitDiscoveryRecord,
  decodeGitEntityRecord,
  decodeGitFetch,
  decodeGitFetchResult,
  decodeGitIndexEntryRecord,
  decodeGitObjectId,
  decodeGitObjectRecord,
  decodeGitOpen,
  decodeGitOpenResult,
  decodeGitProgress,
  decodeGitLogPathRecord,
  decodeGitPatchBaseRecord,
  decodeGitPatchFileRecord,
  decodeGitPatchGapRecord,
  decodeGitPatchRowRecord,
  decodeGitQuery,
  decodeGitQueryCursor,
  decodeGitQueryPage,
  decodeGitQueryState,
  decodeGitReflogRecord,
  decodeGitTreeEntryRecord,
  decodeGitUnwatch,
  decodeGitUnwatchQuery,
  decodeGitWatch,
  decodeGitWatchOptions,
  decodeGitWatchQuery,
  decodeGitWorktreeRecord,
  encodeGitBlameRecord,
  encodeGitClose,
  encodeGitClosed,
  encodeGitCommitRecord,
  encodeGitContentRecord,
  encodeGitDiffRecord,
  encodeGitDiscoveryRecord,
  encodeGitEntityRecord,
  encodeGitFetch,
  encodeGitFetchResult,
  encodeGitIndexEntryRecord,
  encodeGitObjectId,
  encodeGitObjectRecord,
  encodeGitOpen,
  encodeGitOpenResult,
  encodeGitProgress,
  encodeGitLogPathRecord,
  encodeGitPatchBaseRecord,
  encodeGitPatchFileRecord,
  encodeGitPatchGapRecord,
  encodeGitPatchRowRecord,
  encodeGitQuery,
  encodeGitQueryCursor,
  encodeGitQueryPage,
  encodeGitQueryState,
  encodeGitReflogRecord,
  encodeGitTreeEntryRecord,
  encodeGitUnwatch,
  encodeGitUnwatchQuery,
  encodeGitWatch,
  encodeGitWatchOptions,
  encodeGitWatchQuery,
  encodeGitWorktreeRecord,
  decodeExtensions,
  encodeExtensions,
  YasCursor,
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
    "git.object_id.payload",
    (payload) => encodeGitObjectId(decodeGitObjectId(payload)),
  ],
  ["git.open.payload", (payload) => encodeGitOpen(decodeGitOpen(payload))],
  [
    "git.open_terminal.payload",
    (payload) => encodeGitOpen(decodeGitOpen(payload)),
  ],
  [
    "git.open_result.payload",
    (payload) => encodeGitOpenResult(decodeGitOpenResult(payload)),
  ],
  [
    "git.close.payload",
    (payload) => {
      const value = decodeGitClose(payload);
      return encodeGitClose(value.repositoryHandle, value.extensions);
    },
  ],
  [
    "git.closed.payload",
    (payload) => encodeGitClosed(decodeGitClosed(payload)),
  ],
  [
    "git.watch.payload",
    (payload) => {
      const value = decodeGitWatch(payload);
      return encodeGitWatch(
        value.repositoryHandle,
        value.datasets,
        value.encodedStateWatch,
      );
    },
  ],
  [
    "git.unwatch.payload",
    (payload) => encodeGitUnwatch(decodeGitUnwatch(payload)),
  ],
  [
    "git.watch_options.payload",
    (payload) => {
      const value = decodeGitWatch(payload);
      return encodeGitWatch(
        value.repositoryHandle,
        value.datasets,
        value.encodedStateWatch,
      );
    },
  ],
  ["git.query.payload", (payload) => encodeGitQuery(decodeGitQuery(payload))],
  ...[
    "git.resolve_query.payload",
    "git.merge_base_query.payload",
    "git.log_query.payload",
    "git.tree_query.payload",
    "git.blob_query.payload",
    "git.index_query.payload",
    "git.discover_query.payload",
    "git.blame_query.payload",
    "git.reflog_query.payload",
    "git.worktrees_query.payload",
  ].map(
    (name) =>
      [
        name,
        (payload: Uint8Array) => encodeGitQuery(decodeGitQuery(payload)),
      ] as [string, (payload: Uint8Array) => Uint8Array],
  ),
  [
    "git.patch_query.payload",
    (payload) => encodeGitQuery(decodeGitQuery(payload)),
  ],
  [
    "git.watch_query.payload",
    (payload) => {
      const value = decodeGitWatchQuery(payload);
      return encodeGitWatchQuery(
        value.repositoryHandle,
        value.maxRecords,
        value.body,
        value.encodedStateWatch,
      );
    },
  ],
  [
    "git.unwatch_query.payload",
    (payload) => encodeGitUnwatchQuery(decodeGitUnwatchQuery(payload)),
  ],
  ["git.fetch.payload", (payload) => encodeGitFetch(decodeGitFetch(payload))],
  [
    "git.fetch_result.payload",
    (payload) => encodeGitFetchResult(decodeGitFetchResult(payload)),
  ],
  [
    "git.query_page.payload",
    (payload) => encodeGitQueryPage(decodeGitQueryPage(payload)),
  ],
  [
    "git.commit.payload",
    (payload) => encodeGitCommitRecord(decodeGitCommitRecord(payload)),
  ],
  [
    "git.log_path.payload",
    (payload) => encodeGitLogPathRecord(decodeGitLogPathRecord(payload)),
  ],
  [
    "git.patch_file.payload",
    (payload) => encodeGitPatchFileRecord(decodeGitPatchFileRecord(payload)),
  ],
  [
    "git.patch_row.payload",
    (payload) => encodeGitPatchRowRecord(decodeGitPatchRowRecord(payload)),
  ],
  [
    "git.patch_gap.payload",
    (payload) => encodeGitPatchGapRecord(decodeGitPatchGapRecord(payload)),
  ],
  [
    "git.patch_base.payload",
    (payload) => encodeGitPatchBaseRecord(decodeGitPatchBaseRecord(payload)),
  ],
  [
    "git.query_cursor.payload",
    (payload) => encodeGitQueryCursor(decodeGitQueryCursor(payload)),
  ],
  [
    "git.tree_entry.payload",
    (payload) => encodeGitTreeEntryRecord(decodeGitTreeEntryRecord(payload)),
  ],
  [
    "git.blob_content.payload",
    (payload) =>
      encodeGitContentRecord(decodeGitContentRecord(payload, "blob")),
  ],
  [
    "git.diff_record.payload",
    (payload) => encodeGitDiffRecord(decodeGitDiffRecord(payload)),
  ],
  [
    "git.index_record.payload",
    (payload) => encodeGitIndexEntryRecord(decodeGitIndexEntryRecord(payload)),
  ],
  [
    "git.discovery_record.payload",
    (payload) => encodeGitDiscoveryRecord(decodeGitDiscoveryRecord(payload)),
  ],
  [
    "git.blame_record.payload",
    (payload) => encodeGitBlameRecord(decodeGitBlameRecord(payload)),
  ],
  [
    "git.reflog_record.payload",
    (payload) => encodeGitReflogRecord(decodeGitReflogRecord(payload)),
  ],
  [
    "git.worktree_record.payload",
    (payload) => encodeGitWorktreeRecord(decodeGitWorktreeRecord(payload)),
  ],
  [
    "git.entity.payload",
    (payload) => encodeGitEntityRecord(decodeGitEntityRecord(payload)),
  ],
  [
    "git.object_record.payload",
    (payload) => encodeGitObjectRecord(decodeGitObjectRecord(payload)),
  ],
  ...[
    "git.entity.head.payload",
    "git.entity.ref.payload",
    "git.entity.remote.payload",
    "git.entity.operation.payload",
    "git.entity.status.payload",
    "git.entity.upstream.payload",
    "git.entity.stash.payload",
    "git.entity.worktree_generation.payload",
  ].map(
    (name) =>
      [
        name,
        (payload: Uint8Array) =>
          encodeGitEntityRecord(decodeGitEntityRecord(payload)),
      ] as [string, (payload: Uint8Array) => Uint8Array],
  ),
  [
    "git.progress.payload",
    (payload) => encodeGitProgress(decodeGitProgress(payload)),
  ],
  ...["git.query_state.payload", "git.query_state_error.payload"].map(
    (name) =>
      [
        name,
        (payload: Uint8Array) =>
          encodeGitQueryState(decodeGitQueryState(payload)),
      ] as [string, (payload: Uint8Array) => Uint8Array],
  ),
];

describe("YAS Git v1", () => {
  it("round-trips every normative payload and rejects every truncation", () => {
    for (const [name, roundTrip] of cases) {
      const payload = bytes(name);
      expect(roundTrip(payload), name).toEqual(payload);
      for (
        let end = name === "git.query_cursor.payload" ? 1 : 0;
        end < payload.length;
        end++
      )
        expect(
          () => roundTrip(payload.subarray(0, end)),
          `${name}@${end}`,
        ).toThrow(YasProtocolError);
    }
  });
});
