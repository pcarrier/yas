import { describe, expect, it, vi } from "vitest";
import {
  YAS_CHANNEL_OWNER_SESSION,
  YAS_FONT_HARD_LIMITS,
  YAS_RELAY_HARD_LIMITS,
  YAS_STATE_ADD,
  YAS_STATE_SNAPSHOT_BEGIN,
  YAS_STATE_SNAPSHOT_RECORDS,
  YasChannelCatalogue,
  YasExtensionCatalog,
  YasFontCatalog,
  YasLspCatalog,
  YasRelayRoutes,
  YasSelectionCatalog,
  encodeChannelListenerRecord,
  type YasConnection,
  type YasStateBatch,
  type YasTypedRecord,
} from "../yas";

interface CatalogHarness {
  apply(batch: YasStateBatch): void;
}

function connection(): YasConnection {
  return { onInvalidation: vi.fn() } as unknown as YasConnection;
}

function batch(
  phase: number,
  stateRecords: readonly YasTypedRecord[] = [],
): YasStateBatch {
  return {
    phase,
    flags: 0,
    fromRevision: phase === YAS_STATE_SNAPSHOT_BEGIN ? 0n : 1n,
    toRevision: 1n,
    records: stateRecords,
  };
}

function begin(catalog: CatalogHarness): void {
  catalog.apply(batch(YAS_STATE_SNAPSHOT_BEGIN));
}

function records(
  catalog: CatalogHarness,
  stateRecords: readonly YasTypedRecord[] = [],
): void {
  catalog.apply(batch(YAS_STATE_SNAPSHOT_RECORDS, stateRecords));
}

describe("YAS State catalogue admission", () => {
  it("rejects Extension definitions during a rotating multi-batch snapshot", () => {
    const catalog = new YasExtensionCatalog(
      connection(),
      () => undefined,
      () => 1,
    ) as unknown as CatalogHarness;
    begin(catalog);
    const staging = (catalog as unknown as { staging: Map<bigint, unknown> })
      .staging;
    staging.set(1n, { extensionHandle: 1n });
    records(catalog);
    staging.set(2n, { extensionHandle: 2n });
    expect(() => records(catalog)).toThrow(/negotiated definition limit/);
  });

  it("rejects Relay routes before a multi-batch snapshot can keep growing", () => {
    const catalog = new YasRelayRoutes(connection(), () => ({
      ...YAS_RELAY_HARD_LIMITS,
      maxRoutes: 1,
    })) as unknown as CatalogHarness;
    begin(catalog);
    const staging = (catalog as unknown as { staging: Map<bigint, unknown> })
      .staging;
    staging.set(1n, { name: "route-1", flags: 0 });
    records(catalog);
    staging.set(2n, { name: "route-2", flags: 0 });
    expect(() => records(catalog)).toThrow(/negotiated limit/);
  });

  it("checks Font family and face limits on partial snapshots", () => {
    const familyCatalog = new YasFontCatalog(connection(), () => ({
      ...YAS_FONT_HARD_LIMITS,
      maxFamilies: 1,
      maxFacesPerFamily: 1,
    })) as unknown as CatalogHarness;
    begin(familyCatalog);
    const familyStaging = (
      familyCatalog as unknown as { staging: Map<bigint, unknown> }
    ).staging;
    familyStaging.set(1n, { family: "family-1", faceCount: 1 });
    records(familyCatalog);
    familyStaging.set(2n, { family: "family-2", faceCount: 1 });
    expect(() => records(familyCatalog)).toThrow(/negotiated limit/);

    const faceCatalog = new YasFontCatalog(connection(), () => ({
      ...YAS_FONT_HARD_LIMITS,
      maxFacesPerFamily: 1,
    })) as unknown as CatalogHarness;
    begin(faceCatalog);
    (faceCatalog as unknown as { staging: Map<bigint, unknown> }).staging.set(
      1n,
      { family: "family", faceCount: 2 },
    );
    expect(() => records(faceCatalog)).toThrow(/negotiated face limit/);
  });

  it("checks LSP server and per-file diagnostic limits on partial snapshots", () => {
    const serverCatalog = new YasLspCatalog(connection(), 1n, () => ({
      maxServers: 1,
      maxBuffers: 1,
      maxDiagnosticsPerFile: 1,
    })) as unknown as CatalogHarness;
    begin(serverCatalog);
    const serverStaging = (
      serverCatalog as unknown as { staging: Map<string, unknown> }
    ).staging;
    serverStaging.set("server:1", { kind: "backend", value: {} });
    records(serverCatalog);
    serverStaging.set("server:2", { kind: "backend", value: {} });
    expect(() => records(serverCatalog)).toThrow(/negotiated server limit/);

    const diagnosticCatalog = new YasLspCatalog(connection(), 1n, () => ({
      maxServers: 1,
      maxBuffers: 1,
      maxDiagnosticsPerFile: 1,
    })) as unknown as CatalogHarness;
    begin(diagnosticCatalog);
    (
      diagnosticCatalog as unknown as { staging: Map<string, unknown> }
    ).staging.set("diagnostics:path", {
      kind: "diagnostics",
      value: { diagnostics: [{}, {}] },
    });
    expect(() => records(diagnosticCatalog)).toThrow(
      /negotiated per-file limit/,
    );
  });

  it("checks Selection active-drag and item limits on partial snapshots", () => {
    const dragCatalog = new YasSelectionCatalog(connection(), () => ({
      maxActiveDrags: 1,
      maxItems: 1,
    })) as unknown as CatalogHarness;
    begin(dragCatalog);
    const drags = (
      dragCatalog as unknown as {
        staging: { drags: Map<bigint, unknown> };
      }
    ).staging.drags;
    drags.set(1n, { dragHandle: 1n, items: [] });
    records(dragCatalog);
    drags.set(2n, { dragHandle: 2n, items: [] });
    expect(() => records(dragCatalog)).toThrow(/active-drag limit/);

    const itemCatalog = new YasSelectionCatalog(connection(), () => ({
      maxActiveDrags: 1,
      maxItems: 1,
    })) as unknown as CatalogHarness;
    begin(itemCatalog);
    (
      itemCatalog as unknown as {
        staging: { drags: Map<bigint, unknown> };
      }
    ).staging.drags.set(1n, { dragHandle: 1n, items: [{}, {}] });
    expect(() => records(itemCatalog)).toThrow(/negotiated item limit/);
  });

  it("rejects Channel listeners during a rotating multi-batch snapshot", () => {
    const catalog = new YasChannelCatalogue(
      connection(),
      () => 1,
    ) as unknown as CatalogHarness;
    begin(catalog);
    const staging = (catalog as unknown as { staging: Map<bigint, unknown> })
      .staging;
    staging.set(1n, { name: "listener-1" });
    records(catalog);
    staging.set(2n, { name: "listener-2" });
    expect(() => records(catalog)).toThrow(/negotiated listener limit/);
  });

  it("detaches retained Channel records from hostile batch backing buffers", () => {
    const catalog = new YasChannelCatalogue(
      connection(),
      () => 4,
    ) as unknown as CatalogHarness;
    const encoded = encodeChannelListenerRecord({
      listenerHandle: 1n,
      generation: 1n,
      ownerKind: YAS_CHANNEL_OWNER_SESSION,
      ownerSession: new Uint8Array(16).fill(1),
      name: "listener",
      metadata: new Uint8Array([1, 2, 3]),
      extensions: [],
    });
    const hostile = new Uint8Array(encoded.length + 1024 * 1024);
    hostile.set(encoded, 4096);

    begin(catalog);
    records(catalog, [
      {
        kind: YAS_STATE_ADD,
        flags: 0,
        body: hostile.subarray(4096, 4096 + encoded.length),
      },
    ]);

    const retained = (
      catalog as unknown as {
        staging: Map<
          bigint,
          { metadata: Uint8Array; ownerSession: Uint8Array }
        >;
      }
    ).staging.get(1n)!;
    expect(retained.metadata.buffer).not.toBe(hostile.buffer);
    expect(retained.ownerSession.buffer).not.toBe(hostile.buffer);
  });
});
