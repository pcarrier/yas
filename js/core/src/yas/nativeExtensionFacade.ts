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
  type YasExtensionSnapshot,
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

type YasNativeExtensionConnection = Pick<
  YasConnection,
  "family" | "onInvalidation"
>;

interface YasNativeExtensionClient {
  list: YasExtensionClient["list"];
  control: YasExtensionClient["control"];
  deploy: YasExtensionClient["deploy"];
  uploadObject: YasExtensionClient["uploadObject"];
  catalog: {
    readonly snapshot: YasExtensionSnapshot;
    subscribe(listener: (snapshot: YasExtensionSnapshot) => void): () => void;
    unwatch(): Promise<void>;
  };
  dispose?(): void;
}

export class YasNativeExtensionFacade {
  private readonly pendingIdentityCancels = new Set<(error: unknown) => void>();
  private disposed = false;
  readonly connection: YasNativeExtensionConnection;
  readonly client: YasNativeExtensionClient;

  constructor(
    ...args:
      | [connection: YasConnection]
      | [
          connection: YasNativeExtensionConnection,
          client: YasNativeExtensionClient,
        ]
  ) {
    this.connection = args[0];
    this.client =
      args.length === 1 ? new YasExtensionClient(args[0]) : args[1];
  }

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
    let currentHash: Uint8Array | undefined;
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
      currentHash = current.contentHash;
    }

    let module: Uint8Array | undefined;
    const objectBytes = async (): Promise<Uint8Array> => {
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
      return module;
    };
    // A missing object for a create is represented by a temporary NEED_OBJECT
    // definition. An update deliberately keeps the old definition live, so its
    // returned identity cannot carry that phase and used to look like a
    // successful no-op. Stage changed update bytes first; OBJECT_BEGIN is cheap
    // when the server already has them.
    if (currentHash && !sameBytes(currentHash, request.contentHash)) {
      const bytes = await objectBytes();
      await this.client.uploadObject(
        {
          operationId: randomOperationId(),
          contentHash: request.contentHash,
          byteLength: BigInt(bytes.length),
        },
        bytes,
        randomOperationId(),
      );
      this.assertOpen();
    }
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
      if (record.phase !== g.YAS_EXTENSION_PHASE_NEED_OBJECT) {
        if (!sameBytes(record.contentHash, request.contentHash))
          throw new YasProtocolError(
            "Extension update did not adopt the requested object",
          );
        return record;
      }

      const bytes = await objectBytes();
      await this.client.uploadObject(
        {
          operationId: randomOperationId(),
          contentHash: request.contentHash,
          byteLength: BigInt(bytes.length),
        },
        bytes,
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
    if (this.client.dispose) this.client.dispose();
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
      // The request result can overtake the state watch, and the watch can
      // coalesce later lifecycle changes. Treat the returned identity as a
      // lower bound for this handle: wait while the catalogue is behind and
      // accept the current record once it catches up or advances past it.
      if (record.generation < identity.generation) return undefined;
      if (
        record.generation === identity.generation &&
        record.definitionRevision < identity.definitionRevision
      )
        return undefined;
      return record;
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

function sameBytes(left: Uint8Array, right: Uint8Array): boolean {
  return left.length === right.length && left.every((byte, index) => byte === right[index]);
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
