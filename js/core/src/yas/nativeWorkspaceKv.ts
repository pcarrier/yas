/** Purpose-built native YAS KV facade for durable Workspace state. */

import {
  YAS_FAMILY_KV,
  YAS_KV_MAX_INLINE_BYTES,
  YAS_KV_MAX_KEY_BYTES,
} from "./generated";
import {
  YasKvClient,
  YasKvConflictError,
  type YasKvNamespace,
  type YasKvPrecondition,
  type YasKvStateChange,
  type YasKvStateUpdate,
} from "./kv";
import type { YasConnection } from "./session";
import {
  YAS_STATE_DELTA,
  YAS_STATE_RESET,
  YAS_STATE_SNAPSHOT_BEGIN,
  YAS_STATE_SNAPSHOT_END,
  YAS_STATE_SNAPSHOT_RECORDS,
} from "./state";
import { YasProtocolError } from "./wire";
import {
  WorkspaceSessionKvConflictError,
  copyWorkspaceSessionHash,
  type WorkspaceSessionKvDeleteOptions,
  type WorkspaceSessionKvEntry,
  type WorkspaceSessionKvMirror,
  type WorkspaceSessionKvPutOptions,
  type WorkspaceSessionKvWatch,
  type WorkspaceSessionKvWatchOptions,
  type WorkspaceSessionOwnedKv,
} from "../workspaceSessionKv";

const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });

interface ActiveWatch {
  readonly namespace: YasKvNamespace;
  readonly prefix: Uint8Array;
  readonly mirror: WorkspaceSessionKvMirror & {
    readonly live: Map<string, WorkspaceSessionKvEntry>;
  };
  readonly options: WorkspaceSessionKvWatchOptions;
  staging: Map<string, WorkspaceSessionKvEntry> | null;
  closed: boolean;
}

export interface YasNativeWorkspaceKvOptions {
  /** Structural injection seam for focused native persistence tests. */
  client?: Pick<YasKvClient, "open">;
}

/**
 * Keeps native KV identities intact: content hashes remain 32 bytes and watch
 * resources retain their u64 namespace handles. There is no translation
 * adapter or browser-local hash/handle table in this path.
 */
export class YasNativeWorkspaceKv implements WorkspaceSessionOwnedKv {
  private client: Pick<YasKvClient, "open"> | null;
  private removeInvalidation: (() => void) | null;
  private readonly watches = new Set<ActiveWatch>();
  private namespacePromise: Promise<YasKvNamespace> | null = null;
  private generation = 0;
  private disposed = false;

  constructor(
    readonly connection: YasConnection,
    options: YasNativeWorkspaceKvOptions = {},
  ) {
    this.client = options.client ?? null;
    this.removeInvalidation = connection.onInvalidation(({ family, error }) => {
      if (family !== undefined && family !== YAS_FAMILY_KV) return;
      this.resetGeneration(error);
    });
  }

  async kvPut(
    key: string,
    value: Uint8Array,
    options: WorkspaceSessionKvPutOptions = {},
  ): Promise<{ hash: Uint8Array; mtimeNs: bigint }> {
    if (options.create && options.ifHash !== undefined)
      throw new YasProtocolError("KV create and hash preconditions conflict");
    const namespace = await this.namespace();
    try {
      const result = await namespace.put(
        encodeKey(key, false),
        new Uint8Array(value),
        {
          durable: options.durable,
          precondition: mutationPrecondition(options.ifHash, options.create),
        },
      );
      return {
        hash: copyWorkspaceSessionHash(result.contentHash),
        mtimeNs: result.modifiedUnixNs,
      };
    } catch (error) {
      throw mutationError(error);
    }
  }

  async kvDelete(
    key: string,
    options: WorkspaceSessionKvDeleteOptions = {},
  ): Promise<void> {
    const namespace = await this.namespace();
    try {
      await namespace.delete(encodeKey(key, false), {
        durable: options.durable,
        precondition:
          options.ifHash === undefined
            ? { type: "any" }
            : {
                type: "hash",
                contentHash: copyWorkspaceSessionHash(options.ifHash),
              },
      });
    } catch (error) {
      throw mutationError(error);
    }
  }

  async kvFetch(
    key: string,
  ): Promise<{ hash: Uint8Array; value: Uint8Array } | null> {
    const namespace = await this.namespace();
    const result = await namespace.get(encodeKey(key, false));
    return result
      ? {
          hash: copyWorkspaceSessionHash(result.contentHash),
          value: new Uint8Array(result.bytes),
        }
      : null;
  }

  async watchKv(
    prefixText: string,
    options: WorkspaceSessionKvWatchOptions = {},
  ): Promise<WorkspaceSessionKvWatch> {
    this.assertOpen();
    const generation = this.generation;
    const prefix = encodeKey(prefixText, true);
    const namespace = await (await this.clientForUse()).open(prefix);
    if (this.disposed || generation !== this.generation) {
      await namespace.close().catch(() => undefined);
      throw new YasProtocolError(
        "native Workspace KV changed while OPEN was pending",
      );
    }
    const watch: ActiveWatch = {
      namespace,
      prefix,
      mirror: { live: new Map(), snapshotDone: false },
      options,
      staging: null,
      closed: false,
    };
    this.watches.add(watch);
    try {
      await namespace.watch((update) => this.applyUpdate(watch, update), {
        inlineMax:
          options.inlineMax === undefined || options.inlineMax === 0
            ? YAS_KV_MAX_INLINE_BYTES
            : options.inlineMax,
      });
      if (watch.closed || this.disposed || generation !== this.generation) {
        this.closeWatch(watch);
        throw new YasProtocolError(
          "native Workspace KV changed while WATCH was pending",
        );
      }
    } catch (error) {
      this.closeWatch(watch);
      throw error;
    }
    return {
      namespaceHandle: namespace.handle,
      mirror: watch.mirror,
      close: () => this.closeWatch(watch),
    };
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.generation++;
    this.closeGeneration(new Error("native Workspace KV closed"));
    this.removeInvalidation?.();
    this.removeInvalidation = null;
    const client = this.client as { dispose?: () => void } | null;
    client?.dispose?.();
    this.client = null;
  }

  private async namespace(): Promise<YasKvNamespace> {
    this.assertOpen();
    if (!this.namespacePromise) {
      const generation = this.generation;
      const pending = this.clientForUse()
        .then((client) => client.open())
        .then(async (namespace) => {
          if (!this.disposed && generation === this.generation)
            return namespace;
          await namespace.close().catch(() => undefined);
          throw new YasProtocolError(
            "native Workspace KV changed while OPEN was pending",
          );
        });
      this.namespacePromise = pending;
      void pending.catch(() => {
        if (this.namespacePromise === pending) this.namespacePromise = null;
      });
    }
    return this.namespacePromise;
  }

  private async clientForUse(): Promise<Pick<YasKvClient, "open">> {
    // Workspace-session stores start as soon as the authenticated App mounts,
    // while HELLO is still in flight. Family validation is meaningful only
    // after that handshake has installed the negotiated catalogue.
    await this.connection.connect();
    this.assertOpen();
    return (this.client ??= new YasKvClient(this.connection));
  }

  private applyUpdate(watch: ActiveWatch, update: YasKvStateUpdate): void {
    if (watch.closed || this.disposed) return;
    if (update.phase === YAS_STATE_RESET) {
      watch.mirror.live.clear();
      watch.staging?.clear();
      watch.staging = null;
      watch.mirror.snapshotDone = false;
    } else if (update.phase === YAS_STATE_SNAPSHOT_BEGIN) {
      watch.staging = new Map();
    } else if (update.phase === YAS_STATE_SNAPSHOT_RECORDS) {
      if (!watch.staging)
        throw new YasProtocolError("KV snapshot records have no begin");
      applyChanges(watch.prefix, watch.staging, update.changes);
    } else if (update.phase === YAS_STATE_SNAPSHOT_END) {
      if (!watch.staging)
        throw new YasProtocolError("KV snapshot end has no begin");
      applyChanges(watch.prefix, watch.staging, update.changes);
      watch.mirror.live.clear();
      for (const [key, entry] of watch.staging)
        watch.mirror.live.set(key, entry);
      watch.staging = null;
      watch.mirror.snapshotDone = true;
    } else if (update.phase === YAS_STATE_DELTA) {
      applyChanges(watch.prefix, watch.mirror.live, update.changes);
    } else {
      throw new YasProtocolError("unknown KV state phase");
    }
    invokeLifecycleCallback(() => watch.options.onUpdate?.(watch.mirror));
  }

  private closeWatch(watch: ActiveWatch): void {
    if (watch.closed) return;
    watch.closed = true;
    watch.mirror.live.clear();
    watch.staging?.clear();
    watch.staging = null;
    this.watches.delete(watch);
    void watch.namespace.close().catch(() => undefined);
  }

  private resetGeneration(error: Error): void {
    if (this.disposed) return;
    this.generation++;
    this.closeGeneration(error);
  }

  private closeGeneration(error: Error): void {
    for (const watch of [...this.watches]) {
      watch.closed = true;
      watch.mirror.live.clear();
      watch.staging?.clear();
      watch.staging = null;
      invokeLifecycleCallback(() => watch.options.onClosed?.(error));
      void watch.namespace.close().catch(() => undefined);
    }
    this.watches.clear();
    const namespace = this.namespacePromise;
    this.namespacePromise = null;
    if (namespace)
      void namespace.then((value) => value.close()).catch(() => {});
  }

  private assertOpen(): void {
    if (this.disposed)
      throw new YasProtocolError("native Workspace KV is closed");
  }
}

function invokeLifecycleCallback(callback: (() => void) | undefined): void {
  if (!callback) return;
  try {
    callback();
  } catch (error) {
    reportLifecycleError(error);
  }
}

function reportLifecycleError(error: unknown): void {
  try {
    const report = (
      globalThis as typeof globalThis & {
        reportError?: (value: unknown) => void;
      }
    ).reportError;
    if (report) report(error);
    else console.error("YAS KV lifecycle callback failed", error);
  } catch {
    // Cleanup must not depend on host error reporting.
  }
}

function mutationPrecondition(
  ifHash: Uint8Array | undefined,
  create: boolean | undefined,
): YasKvPrecondition {
  if (create) return { type: "absent" };
  return ifHash === undefined
    ? { type: "any" }
    : { type: "hash", contentHash: copyWorkspaceSessionHash(ifHash) };
}

function mutationError(error: unknown): Error {
  if (error instanceof YasKvConflictError)
    return new WorkspaceSessionKvConflictError(error.result.contentHash);
  return error instanceof Error ? error : new Error(String(error));
}

function applyChanges(
  prefix: Uint8Array,
  target: Map<string, WorkspaceSessionKvEntry>,
  changes: readonly YasKvStateChange[],
): void {
  for (const change of changes) {
    if (change.type === "remove") {
      target.delete(decodeFullKey(prefix, change.relativeKey));
      continue;
    }
    const entry = change.entry;
    target.set(decodeFullKey(prefix, entry.relativeKey), {
      hash: copyWorkspaceSessionHash(entry.contentHash),
      size: safeLength(entry.byteLength),
      mtimeNs: entry.modifiedUnixNs,
      value:
        entry.inlineValue === undefined
          ? null
          : new Uint8Array(entry.inlineValue),
    });
  }
}

function encodeKey(value: string, allowEmpty: boolean): Uint8Array {
  const bytes = encoder.encode(value);
  if (
    (!allowEmpty && bytes.length === 0) ||
    value.includes("\0") ||
    bytes.length > YAS_KV_MAX_KEY_BYTES
  )
    throw new YasProtocolError("invalid Workspace KV key");
  return bytes;
}

function decodeFullKey(prefix: Uint8Array, relative: Uint8Array): string {
  const bytes = new Uint8Array(prefix.length + relative.length);
  bytes.set(prefix);
  bytes.set(relative, prefix.length);
  const value = decoder.decode(bytes);
  if (value.includes("\0"))
    throw new YasProtocolError("invalid Workspace KV key");
  return value;
}

function safeLength(value: bigint): number {
  if (value > BigInt(Number.MAX_SAFE_INTEGER))
    throw new YasProtocolError("Workspace KV value exceeds browser precision");
  return Number(value);
}
