import type { YasWorkspace, FsFileIndex } from "@yas-run/core";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  FILE_INDEX_CACHE_MAX_BYTES,
  FILE_INDEX_CACHE_MAX_ITEMS,
  FILE_INDEX_MAX_INFLIGHT,
  FILE_INDEX_MAX_PATHS,
  dropFileIndexes,
  fileIndexCacheStats,
  localFileIndex,
  resetFileIndexCache,
  searchFileIndex,
} from "../ide/fileIndex";

function workspace(
  indexFiles: (root: string) => Promise<FsFileIndex>,
): YasWorkspace {
  return {
    indexFiles: (_connectionId: string, root: string) => indexFiles(root),
  } as unknown as YasWorkspace;
}

afterEach(() => {
  resetFileIndexCache();
});

describe("local file-index cache bounds", () => {
  it("evicts old connection/root keys under hostile rotation", async () => {
    const indexFiles = vi.fn(async (root: string) => ({
      paths: [`${root}/file.ts`],
      truncated: false,
    }));
    const ws = workspace(indexFiles);
    for (let i = 0; i < FILE_INDEX_CACHE_MAX_ITEMS + 4; i++) {
      localFileIndex(ws, "peer", `/root-${i}`);
      await vi.waitFor(() => expect(fileIndexCacheStats().inflight).toBe(0));
    }
    const stats = fileIndexCacheStats();
    expect(stats.items).toBe(FILE_INDEX_CACHE_MAX_ITEMS);
    expect(stats.bytes).toBeLessThanOrEqual(FILE_INDEX_CACHE_MAX_BYTES);
  });

  it("keeps an oversized index only as a zero-byte retry tombstone", async () => {
    const indexFiles = vi.fn(async () => ({
      paths: Array(FILE_INDEX_MAX_PATHS + 1).fill("a"),
      truncated: true,
    }));
    const ws = workspace(indexFiles);
    expect(localFileIndex(ws, "peer", "/huge")).toBeNull();
    await vi.waitFor(() => expect(fileIndexCacheStats().inflight).toBe(0));
    expect(localFileIndex(ws, "peer", "/huge")).toBeNull();
    expect(indexFiles).toHaveBeenCalledTimes(1);
    expect(fileIndexCacheStats()).toMatchObject({ items: 1, bytes: 0 });
  });

  it("caps concurrent index fetches and detaches evicted replies", async () => {
    const resolves: Array<(index: FsFileIndex) => void> = [];
    const indexFiles = vi.fn(
      () =>
        new Promise<FsFileIndex>((resolve) => {
          resolves.push(resolve);
        }),
    );
    const ws = workspace(indexFiles);
    for (let i = 0; i < FILE_INDEX_CACHE_MAX_ITEMS + 8; i++) {
      localFileIndex(ws, "peer", `/pending-${i}`);
    }
    expect(indexFiles).toHaveBeenCalledTimes(FILE_INDEX_MAX_INFLIGHT);
    expect(fileIndexCacheStats().items).toBe(FILE_INDEX_CACHE_MAX_ITEMS);

    dropFileIndexes("peer");
    for (const resolve of resolves) {
      resolve({ paths: ["late.ts"], truncated: false });
    }
    await vi.waitFor(() => expect(fileIndexCacheStats().inflight).toBe(0));
    expect(fileIndexCacheStats()).toEqual({ items: 0, bytes: 0, inflight: 0 });
  });
});

describe("local file-index ranking parity", () => {
  it("uses the native byte-span scorer without basename bonuses", () => {
    const index = {
      paths: ["x/f------o", "fo/xxxxxxxxxxxxxxxxxxxx"],
      truncated: false,
    };
    expect(searchFileIndex(index, "fo", 10)).toEqual([
      "fo/xxxxxxxxxxxxxxxxxxxx",
      "x/f------o",
    ]);
  });

  it("ASCII-folds exactly and preserves non-ASCII byte case", () => {
    const index = {
      paths: ["UPPER.txt", "Ä.txt", "ä.txt"],
      truncated: false,
    };
    expect(searchFileIndex(index, "upper", 10)).toEqual(["UPPER.txt"]);
    expect(searchFileIndex(index, "Ä", 10)).toEqual(["Ä.txt"]);
  });

  it("uses recency only after native fuzzy span", () => {
    const index = {
      paths: ["very/long/path/fo", "foo"],
      truncated: false,
    };
    expect(
      searchFileIndex(index, "fo", 10, (path) =>
        path === "very/long/path/fo" ? 0 : null,
      ),
    ).toEqual(["very/long/path/fo", "foo"]);
  });
});
