import { describe, expect, it, vi } from "vitest";
import {
  YAS_EXTENSION_CONTROL_RESTART,
  YAS_EXTENSION_PHASE_RUNNING,
} from "../yas/generated";
import type { YasExtensionClient, YasExtensionRecord } from "../yas/extension";
import { YasNativeExtensionFacade } from "../yas/nativeExtensionFacade";
import type { YasConnection } from "../yas/session";

function nativeRecord(): YasExtensionRecord {
  return {
    extensionHandle: 0xfedc_ba98_7654_3210n,
    generation: 0x8000_0000_0000_0011n,
    definitionRevision: 0x8000_0000_0000_0022n,
    phase: YAS_EXTENSION_PHASE_RUNNING,
    runtime: 1,
    restartPolicy: 2,
    flags: 15,
    attempt: 0x8000_0000_0000_0033n,
    lastRunningAttempt: 0x8000_0000_0000_0033n,
    taskId: 7,
    nextStartUnixMs: 0n,
    directoryRevision: 4n,
    contentHash: new Uint8Array(32).fill(7),
    name: "systemd",
    runtimeLimits: {
      memoryBytes: 0n,
      stackBytes: 0n,
      maxActiveJobs: 0,
      maxPendingJobs: 0,
      maxJobBytes: 0n,
      slowConsumerTimeoutNs: 0n,
    },
    extensions: [],
  };
}

describe("YasNativeExtensionFacade", () => {
  it("controls the exact native handle, generation, and revision", async () => {
    const record = nativeRecord();
    const control = vi.fn().mockResolvedValue({
      extensionHandle: record.extensionHandle,
      generation: record.generation,
      definitionRevision: record.definitionRevision,
      extensions: [],
    });
    const client = {
      list: vi.fn().mockResolvedValue({ revision: 1n, definitions: [record] }),
      control,
      catalog: {
        snapshot: { revision: 1n, definitions: [record] },
        subscribe: vi.fn(() => () => undefined),
        unwatch: vi.fn(),
      },
    } as unknown as YasExtensionClient;
    const connection = {
      onInvalidation: vi.fn(() => () => undefined),
    } as unknown as YasConnection;
    const facade = new YasNativeExtensionFacade(connection, client);

    const result = await facade.controlExtension(
      record.extensionHandle,
      YAS_EXTENSION_CONTROL_RESTART,
    );
    expect(result).toBe(record);
    expect(control).toHaveBeenCalledWith(
      expect.objectContaining({
        extensionHandle: record.extensionHandle,
        generation: record.generation,
        expectedDefinitionRevision: record.definitionRevision,
        action: YAS_EXTENSION_CONTROL_RESTART,
      }),
    );
  });

  it("deploys an update with its full high-bit CAS identity", async () => {
    const record = nativeRecord();
    const deploy = vi.fn().mockResolvedValue({
      extensionHandle: record.extensionHandle,
      generation: record.generation,
      definitionRevision: record.definitionRevision,
      extensions: [],
    });
    const client = {
      list: vi.fn().mockResolvedValue({ revision: 1n, definitions: [record] }),
      deploy,
      catalog: {
        snapshot: { revision: 1n, definitions: [record] },
        subscribe: vi.fn(() => () => undefined),
        unwatch: vi.fn(),
      },
    } as unknown as YasExtensionClient;
    const connection = {
      onInvalidation: vi.fn(() => () => undefined),
    } as unknown as YasConnection;
    const facade = new YasNativeExtensionFacade(connection, client);

    await facade.installExtension({
      contentHash: record.contentHash,
      name: record.name,
      expectedExtensionHandle: record.extensionHandle,
      expectedGeneration: record.generation,
      expectedDefinitionRevision: record.definitionRevision,
      module: vi.fn(() => Promise.reject(new Error("object not requested"))),
    });
    expect(deploy).toHaveBeenCalledWith(
      expect.objectContaining({
        expectedExtensionHandle: record.extensionHandle,
        expectedGeneration: record.generation,
        expectedDefinitionRevision: record.definitionRevision,
      }),
    );
  });
});
