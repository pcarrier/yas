/** Browser product operations over the typed YAS Extension family.
 *
 * Definitions retain their native bigint handle, generation, revision, phase,
 * flags, and 32-byte content hash.
 */

import * as g from "./generated";
import {
  type YasExtensionDefinitionIdentity,
  type YasExtensionRecord,
  type YasExtensionRuntimeLimits,
  YasExtensionClient,
  extensionLimitsFromExtensions,
} from "./extension";
import type { YasConnection } from "./session";
import { YasDisconnectedError, YasProtocolError } from "./wire";

const encoder = new TextEncoder();

export interface YasNativeExtensionInstallRequest {
  contentHash: Uint8Array;
  name: string;
  module: () => Promise<Uint8Array>;
  args?: readonly string[];
  flags?: number;
  runtime?: number;
  restartPolicy?: number;
  runtimeLimits?: YasExtensionRuntimeLimits;
  expectedExtensionHandle?: bigint;
  expectedGeneration?: bigint;
  expectedDefinitionRevision?: bigint;
}

export class YasNativeExtensionFacade {
  private readonly pendingIdentityCancels = new Set<(error: unknown) => void>();
  private disposed = false;

  constructor(
    readonly connection: YasConnection,
    readonly client: YasExtensionClient = new YasExtensionClient(connection),
  ) {}

  async listExtensions(): Promise<readonly YasExtensionRecord[]> {
    this.assertOpen();
    return (await this.client.list()).definitions;
  }

  async controlExtension(
    extensionHandle: bigint,
    action: number,
  ): Promise<YasExtensionRecord | null> {
    this.assertOpen();
    const current = await this.find(extensionHandle);
    this.assertOpen();
    const identity = await this.client.control({
      extensionHandle: current.extensionHandle,
      generation: current.generation,
      expectedDefinitionRevision: current.definitionRevision,
      operationId: randomOperationId(),
      action,
    });
    if (action === g.YAS_EXTENSION_CONTROL_REMOVE) return null;
    return this.waitForIdentity(identity);
  }

  async installExtension(
    request: YasNativeExtensionInstallRequest,
  ): Promise<YasExtensionRecord> {
    this.assertOpen();
    if (
      request.contentHash.length !== 32 ||
      request.contentHash.every((byte) => byte === 0)
    )
      throw new YasProtocolError("Extension content hash is invalid");
    await this.client.list();
    this.assertOpen();

    const explicitUpdate = request.expectedExtensionHandle !== undefined;
    let expectedHandle = 0n;
    let expectedGeneration = 0n;
    let expectedRevision = request.expectedDefinitionRevision ?? 0n;
    if (explicitUpdate) {
      const current = await this.find(request.expectedExtensionHandle!);
      if (
        (request.expectedGeneration !== undefined &&
          current.generation !== request.expectedGeneration) ||
        current.definitionRevision !== expectedRevision ||
        current.name !== request.name
      )
        throw new YasProtocolError("Extension update identity is stale");
      expectedHandle = current.extensionHandle;
      expectedGeneration = current.generation;
    }

    let module: Uint8Array | undefined;
    for (let attempt = 0; attempt < 3; attempt++) {
      const identity = await this.client.deploy({
        operationId: randomOperationId(),
        expectedExtensionHandle: expectedHandle,
        expectedGeneration,
        expectedDefinitionRevision: expectedRevision,
        flags:
          request.flags ??
          g.YAS_EXTENSION_DEFINITION_PERSISTENT |
            g.YAS_EXTENSION_DEFINITION_ENABLED |
            g.YAS_EXTENSION_DEFINITION_DESIRED_RUNNING |
            g.YAS_EXTENSION_DEFINITION_DETACHED,
        runtime: request.runtime ?? g.YAS_EXTENSION_RUNTIME_WASMI,
        restartPolicy: request.restartPolicy ?? g.YAS_EXTENSION_RESTART_ALWAYS,
        name: request.name,
        contentHash: new Uint8Array(request.contentHash),
        argv: (request.args ?? []).map((value) => encoder.encode(value)),
        runtimeLimits: request.runtimeLimits ?? defaultRuntimeLimits(),
      });
      this.assertOpen();
      const record = await this.waitForIdentity(identity);
      if (record.phase !== g.YAS_EXTENSION_PHASE_NEED_OBJECT) return record;

      module ??= new Uint8Array(await request.module());
      if (
        module.length === 0 ||
        BigInt(module.length) >
          extensionLimitsFromExtensions(
            this.connection.family(
              g.YAS_FAMILY_EXTENSION,
              g.YAS_EXTENSION_VERSION,
            ).limits,
          ).maxObjectBytes
      )
        throw new YasProtocolError(
          "Extension object is empty or exceeds the negotiated limit",
        );
      await this.client.uploadObject(
        {
          operationId: randomOperationId(),
          contentHash: request.contentHash,
          byteLength: BigInt(module.length),
        },
        module,
        randomOperationId(),
      );
      this.assertOpen();
      expectedHandle = record.extensionHandle;
      expectedGeneration = record.generation;
      expectedRevision = record.definitionRevision;
    }
    throw new YasProtocolError("server kept requesting the Extension object");
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    const error = new YasDisconnectedError("Extension facade is disposed");
    for (const cancel of [...this.pendingIdentityCancels]) cancel(error);
    this.pendingIdentityCancels.clear();
    const disposable = this.client as YasExtensionClient & {
      dispose?: () => void;
    };
    if (disposable.dispose) disposable.dispose();
    else void this.client.catalog.unwatch().catch(() => undefined);
  }

  private async find(extensionHandle: bigint): Promise<YasExtensionRecord> {
    this.assertOpen();
    if (extensionHandle === 0n)
      throw new YasProtocolError("Extension handle is zero");
    const snapshot = await this.client.list();
    this.assertOpen();
    const record = snapshot.definitions.find(
      (candidate) => candidate.extensionHandle === extensionHandle,
    );
    if (!record) throw new YasProtocolError("unknown Extension identity");
    return record;
  }

  private waitForIdentity(
    identity: YasExtensionDefinitionIdentity,
  ): Promise<YasExtensionRecord> {
    this.assertOpen();
    const match = (definitions: readonly YasExtensionRecord[]) => {
      const record = definitions.find(
        (candidate) => candidate.extensionHandle === identity.extensionHandle,
      );
      if (!record) return undefined;
      if (
        record.generation !== identity.generation ||
        record.definitionRevision > identity.definitionRevision
      )
        throw new YasProtocolError(
          "Extension identity advanced before delivery",
        );
      return record.definitionRevision === identity.definitionRevision
        ? record
        : undefined;
    };
    const immediate = match(this.client.catalog.snapshot.definitions);
    if (immediate) return Promise.resolve(immediate);
    return new Promise((resolve, reject) => {
      let removeCatalog: (() => void) | undefined;
      let removeInvalidation: (() => void) | undefined;
      let settled = false;
      const finish = (record?: YasExtensionRecord, error?: unknown) => {
        if (settled) return;
        settled = true;
        removeCatalog?.();
        removeInvalidation?.();
        this.pendingIdentityCancels.delete(cancel);
        if (error !== undefined) reject(error);
        else resolve(record!);
      };
      const cancel = (error: unknown) => finish(undefined, error);
      this.pendingIdentityCancels.add(cancel);
      removeCatalog = this.client.catalog.subscribe((snapshot) => {
        try {
          const record = match(snapshot.definitions);
          if (record) finish(record);
        } catch (error) {
          finish(undefined, error);
        }
      });
      removeInvalidation = this.connection.onInvalidation(({ family }) => {
        if (family === undefined || family === g.YAS_FAMILY_EXTENSION)
          finish(
            undefined,
            new YasDisconnectedError("Extension state invalidated"),
          );
      });
    });
  }

  private assertOpen(): void {
    if (this.disposed)
      throw new YasProtocolError("Extension facade is disposed");
  }
}

export function yasExtensionHashHex(bytes: Uint8Array): string {
  if (bytes.length !== 32)
    throw new YasProtocolError("Extension content hash is not 32 bytes");
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(
    "",
  );
}

function defaultRuntimeLimits(): YasExtensionRuntimeLimits {
  return {
    memoryBytes: 0n,
    stackBytes: 0n,
    maxActiveJobs: 0,
    maxPendingJobs: 0,
    maxJobBytes: 0n,
    slowConsumerTimeoutNs: 0n,
  };
}

let fallbackOperationId = 1n;
function randomOperationId(): Uint8Array {
  const value = new Uint8Array(16);
  globalThis.crypto?.getRandomValues(value);
  if (value.every((byte) => byte === 0)) {
    new DataView(value.buffer).setBigUint64(8, fallbackOperationId++, true);
  }
  return value;
}
