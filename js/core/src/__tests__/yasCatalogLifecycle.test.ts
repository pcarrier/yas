import { describe, expect, it, vi } from "vitest";
import {
  YAS_STATE_SNAPSHOT_BEGIN,
  YAS_STATE_SNAPSHOT_END,
  YAS_STATE_RESET,
  YasClientCatalog,
  YasDesktopCatalog,
  YasMediaCatalog,
  YasSelectionCatalog,
  YasSurfaceCatalog,
  YasTerminalCatalog,
  YasTerminalView,
  YasStateCatalogueRetention,
  detachStateRetainedValue,
  type YasConnection,
  type YasStateBatch,
} from "../yas";

interface CatalogHarness {
  subscription: { active: boolean; unwatch(): Promise<void> } | null;
  apply(batch: YasStateBatch): void;
  applyRecords: ReturnType<typeof vi.fn>;
  validateCatalog: ReturnType<typeof vi.fn>;
  unwatch(): Promise<void>;
}

const constructors = [
  YasTerminalCatalog,
  YasClientCatalog,
  YasSurfaceCatalog,
  YasSelectionCatalog,
  YasDesktopCatalog,
  YasMediaCatalog,
] as const;

function batch(
  phase: number,
  records: YasStateBatch["records"] = [],
): YasStateBatch {
  return {
    phase,
    flags: 0,
    fromRevision: 0n,
    toRevision: phase === YAS_STATE_SNAPSHOT_END ? 1n : 0n,
    records,
  };
}

describe("YAS state catalogue lifecycle", () => {
  for (const Catalog of constructors) {
    it(`${Catalog.name} preserves WATCH ownership across RESET and consumes END records`, async () => {
      const connection = {
        onInvalidation: vi.fn(),
      } as unknown as YasConnection;
      const catalog = new Catalog(connection) as unknown as CatalogHarness;
      const subscription = {
        active: true,
        unwatch: vi.fn().mockResolvedValue(undefined),
      };
      const applyRecords = vi.fn();
      const validateCatalog = vi.fn();
      Object.assign(catalog, { subscription, applyRecords, validateCatalog });

      catalog.apply(batch(YAS_STATE_RESET));
      expect(catalog.subscription).toBe(subscription);

      catalog.apply(batch(YAS_STATE_SNAPSHOT_BEGIN));
      const records = [
        { kind: 0, flags: 0, body: new Uint8Array([1]) },
      ] as const;
      catalog.apply(batch(YAS_STATE_SNAPSHOT_END, records));
      expect(applyRecords).toHaveBeenCalledTimes(1);
      expect(applyRecords.mock.calls[0]!.at(-1)).toBe(records);

      await catalog.unwatch();
      expect(subscription.unwatch).toHaveBeenCalledTimes(1);
    });
  }

  it("keeps Terminal records through the RESET that precedes a republication", () => {
    const connection = {
      onInvalidation: vi.fn(),
    } as unknown as YasConnection;
    const catalog = new YasTerminalCatalog(connection);
    const harness = catalog as unknown as CatalogHarness;
    Object.assign(harness, {
      subscription: { active: true, unwatch: vi.fn() },
      applyRecords: (target: Map<bigint, unknown>) =>
        target.set(1n, { terminalHandle: 1n }),
      validateCatalog: vi.fn(),
    });

    harness.apply(batch(YAS_STATE_SNAPSHOT_BEGIN));
    harness.apply(batch(YAS_STATE_SNAPSHOT_END));
    expect(catalog.snapshot.terminals).toHaveLength(1);

    const seen: number[] = [];
    catalog.subscribe((snapshot) => seen.push(snapshot.terminals.length));
    expect(seen).toEqual([1]);

    // The server sends RESET before every republication of the catalogue. It
    // is not a moment with no terminals: an empty snapshot here unmounts every
    // terminal pane in the UI and rebuilds it when the records arrive.
    harness.apply(batch(YAS_STATE_RESET));
    expect(catalog.snapshot.terminals).toHaveLength(1);
    expect(seen).toEqual([1]);

    harness.apply(batch(YAS_STATE_SNAPSHOT_BEGIN));
    harness.apply(batch(YAS_STATE_SNAPSHOT_END));
    expect(seen).toEqual([1, 1]);
  });

  it("releases queued Terminal frames when a view is invalidated", () => {
    const release = vi.fn();
    const view = new YasTerminalView(
      { removeView: vi.fn() } as never,
      {
        viewId: 7,
        codecVersion: 1,
        maxInflightFrames: 2,
        maxEncodedFrame: 1024,
        maxDecodedFrame: 1024,
        firstSequence: 1,
        extensions: [],
      },
      { bytes: 1024n, release } as never,
    );
    view.acceptFrame({
      viewId: 7,
      sequence: 1,
      flags: 0,
      gridPayload: new Uint8Array([1]),
    });
    expect(
      (view as unknown as { pendingFrames: unknown[] }).pendingFrames,
    ).toHaveLength(1);

    view.closeLocal();

    expect(
      (view as unknown as { pendingFrames: unknown[] }).pendingFrames,
    ).toHaveLength(0);
    expect(release).toHaveBeenCalledOnce();
  });

  it("keeps decoded State byte admission transactional", () => {
    const retention = new YasStateCatalogueRetention<string>(512);
    retention.upsert("first", 128);
    const before = retention.bytes;

    expect(() => retention.upsert("second", 320)).toThrow(/retained byte/);
    expect(retention.bytes).toBe(before);

    const replacement = retention.clone();
    replacement.upsert("first", 1);
    expect(replacement.bytes).toBeLessThan(retention.bytes);
    expect(retention.bytes).toBe(before);
    replacement.dispose();
    retention.dispose();
  });

  it("caps retained State item count independently of bytes", () => {
    const retention = new YasStateCatalogueRetention<number>(1024);
    for (let index = 0; index < 8; index++) retention.upsert(index, 0);

    expect(() => retention.upsert(8, 0)).toThrow(/retained item limit/);
    expect(retention.bytes).toBe(8 * 64);
    retention.dispose();
  });

  it("detaches retained byte views from a larger peer frame", () => {
    const frame = new Uint8Array(1024 * 1024);
    const retained = detachStateRetainedValue({
      value: frame.subarray(123, 127),
      nested: [{ value: frame.subarray(456, 458) }],
    });

    expect(retained.value).toEqual(new Uint8Array(4));
    expect(retained.value.buffer.byteLength).toBe(4);
    expect(retained.nested[0]!.value.buffer.byteLength).toBe(2);
  });

  it("shares retained admission across catalogues on one connection", () => {
    const connection = {} as YasConnection;
    const first = YasStateCatalogueRetention.forConnection<string>(connection);
    const overlappingWatch =
      YasStateCatalogueRetention.forConnection<string>(connection);
    const halfPool = 128 * 1024 * 1024;

    first.upsert("first", halfPool);
    expect(() => overlappingWatch.upsert("second", halfPool)).toThrow(
      /connection retained byte limit/,
    );
    expect(overlappingWatch.bytes).toBe(0);

    first.dispose();
    expect(() => overlappingWatch.upsert("second", halfPool)).not.toThrow();
    overlappingWatch.dispose();
  });

  it("reserves a cloned generation before catalogue allocation", () => {
    const connection = {} as YasConnection;
    const current =
      YasStateCatalogueRetention.forConnection<string>(connection);
    const other = YasStateCatalogueRetention.forConnection<string>(connection);
    current.upsert("large", 140 * 1024 * 1024);

    expect(() => current.clone()).toThrow(/connection retained byte limit/);
    expect(current.bytes).toBeGreaterThan(0);
    expect(() => other.upsert("other", 100 * 1024 * 1024)).not.toThrow();

    current.dispose();
    other.dispose();
  });
});
