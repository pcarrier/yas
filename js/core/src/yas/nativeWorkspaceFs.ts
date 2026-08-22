/** Workspace-facing filesystem operations backed directly by typed YAS FS. */

import {
  FS_CLOSED_CLIENT_REQUEST,
  FS_CLOSED_CONNECTION_LOST,
  FS_ENTRY_DIR,
  FS_ENTRY_FILE,
  FS_ENTRY_FILTERED,
  FS_ENTRY_LINK_DIR,
  FS_ENTRY_NO_CONTENT,
  FS_ENTRY_SYMLINK,
  FS_ENTRY_UNREADABLE,
  FS_ENTRY_UNSTABLE,
  type FsFileIndex,
  type FsGrepOptions,
  type FsGrepResult,
  type FsSyncOptions,
} from "../fsModel";
import { Notifier } from "../reactive";
import type { SessionId } from "../types";
import * as g from "./generated";
import {
  YasFsClient,
  decodeFsConflictDetail,
  decodeFsEntry,
  decodeFsEntryPatch,
  decodeFsMove,
  decodeFsQueryRecord,
  decodeFsRemoveRecord,
  type YasFsApplyItem,
  type YasFsApplyItemResult,
  type YasFsEntryRecord,
  type YasFsPath,
  type YasFsPrecondition,
  type YasFsQueryRecord,
  type YasFsRoot,
  type YasFsRootSource,
} from "./fs";
import type { YasConnection } from "./session";
import {
  YAS_STATE_ADD,
  YAS_STATE_DELTA,
  YAS_STATE_PATCH,
  YAS_STATE_REMOVE,
  YAS_STATE_REPLACE,
  YAS_STATE_RESET,
  YAS_STATE_SNAPSHOT_BEGIN,
  YAS_STATE_SNAPSHOT_END,
  YAS_STATE_SNAPSHOT_RECORDS,
  type YasStateBatch,
} from "./state";
import { YasProtocolError, YasResultError } from "./wire";

const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });
const DEFAULT_QUERY_CREDIT = 4n * 1024n * 1024n;
const DEFAULT_UPLOAD_CREDIT = 1024n * 1024n;
const DEFAULT_UPLOAD_CHUNK = 256 * 1024;
const MAX_INDEX_PATHS = 1_000_000;
const ZERO_HASH = new Uint8Array(32);
// Pending watch correlations cover one server APPLY batch plus every staging
// slot. Once the bounded budget is full, fail admission before retaining a new
// operation/path pair. Self-echo hashes use the APPLY item ceiling because
// they are only a best-effort bridge until the matching State delta arrives.
const MAX_PENDING_MUTATIONS =
  g.YAS_FS_MAX_BATCH_ITEMS + g.YAS_FS_MAX_STAGES_PER_SESSION;
const MAX_REMEMBERED_WRITES = g.YAS_FS_MAX_BATCH_ITEMS;

export interface YasNativeFsNode {
  entryFlags: number;
  size: number;
  mtimeNs: bigint;
  mode: number;
  /** Exact 32-byte BLAKE3 hash. Directories use the protocol zero value. */
  hash: Uint8Array;
  content: Uint8Array | null;
}

export type YasNativeFsRecord =
  | {
      kind: "upsert";
      path: string;
      entryFlags: number;
      size: number;
      mtimeNs: bigint;
      mode: number;
      hash: Uint8Array;
      content: { kind: "none" } | { kind: "full"; data: Uint8Array };
    }
  | { kind: "delete"; path: string }
  | { kind: "move"; from: string; to: string };

export interface YasNativeFsSyncOptions extends Omit<
  FsSyncOptions,
  "onRecord"
> {
  onRecord?: (record: YasNativeFsRecord) => void;
}

export interface YasNativeFsWriteOptions {
  ifHash?: Uint8Array;
  deltaBase?: Uint8Array;
  create?: boolean;
  force?: boolean;
  mode?: number;
  createParents?: boolean;
  durable?: boolean;
}

export interface YasNativeFsUploadOptions extends YasNativeFsWriteOptions {
  chunkSize?: number;
  onProgress?: (uploaded: number, total: number) => void;
  signal?: AbortSignal;
}

export interface YasNativeFsLinkOptions {
  ifHash?: Uint8Array;
  force?: boolean;
  createParents?: boolean;
}

export interface YasNativeFsWriteResult {
  hash: Uint8Array;
  mtimeNs: bigint;
}

export interface YasNativeFsUploadResult extends YasNativeFsWriteResult {
  mtime: number;
}

export interface YasNativeFsSyncHandle {
  /** Server-issued opaque root identity, never projected to a number. */
  readonly rootHandle: bigint;
  readonly root: string;
  readonly live: ReadonlyMap<string, YasNativeFsNode>;
  readonly revision: number;
  subscribe(listener: () => void): () => void;
  fetch(path: string): Promise<Uint8Array>;
  writeFile(
    path: string,
    data: Uint8Array,
    options?: YasNativeFsWriteOptions,
  ): Promise<YasNativeFsWriteResult>;
  upload(
    path: string,
    data: Uint8Array | Blob,
    options?: YasNativeFsUploadOptions,
  ): Promise<YasNativeFsUploadResult>;
  mkdir(
    path: string,
    options?: { mode?: number; createParents?: boolean },
  ): Promise<YasNativeFsWriteResult>;
  remove(path: string, options?: { ifHash?: Uint8Array }): Promise<void>;
  rename(
    from: string,
    to: string,
    options?: { createParents?: boolean },
  ): Promise<void>;
  symlink(
    target: string,
    path: string,
    options?: YasNativeFsLinkOptions,
  ): Promise<YasNativeFsWriteResult>;
  hardlink(
    source: string,
    path: string,
    options?: YasNativeFsLinkOptions,
  ): Promise<YasNativeFsWriteResult>;
  lastWrittenHash(path: string): Uint8Array | undefined;
  stop(): void;
}

export class YasNativeFsConflictError extends Error {
  readonly hash: Uint8Array;

  constructor(hash: Uint8Array) {
    super("filesystem write conflict");
    this.name = "YasNativeFsConflictError";
    this.hash = exactHash(hash);
  }
}

/** The native FS backend reserves common status IO for permission denial;
 * other operating-system I/O failures are reported as INTERNAL. */
export class YasNativeFsPermissionError extends Error {
  constructor(readonly detail: string) {
    super(detail || "filesystem operation is not permitted");
    this.name = "YasNativeFsPermissionError";
  }
}

export interface YasNativeWorkspaceFsOptions {
  terminalHandle(sessionId: SessionId): bigint | undefined;
  client?: Pick<YasFsClient, "open">;
  hashBytes?: (bytes: Uint8Array) => Uint8Array | Promise<Uint8Array>;
}

export class YasNativeWorkspaceFs {
  private client: Pick<YasFsClient, "open"> | null;
  private readonly hashBytes: NonNullable<
    YasNativeWorkspaceFsOptions["hashBytes"]
  >;
  private readonly handles = new Set<NativeFsHandle>();
  private readonly removeInvalidation: () => void;
  private generation = 0;
  private disposed = false;

  constructor(
    readonly connection: YasConnection,
    private readonly options: YasNativeWorkspaceFsOptions,
  ) {
    // Family clients validate the negotiated catalogue in their constructor.
    // Keep that validation at first use so a Workspace can be assembled before
    // HELLO without treating an unadvertised optional family as fatal.
    this.client = options.client ?? null;
    this.hashBytes = options.hashBytes ?? defaultBlake3;
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family !== undefined && family !== g.YAS_FAMILY_FS) return;
      this.generation++;
      for (const handle of [...this.handles]) handle.invalidate();
    });
  }

  async syncFs(
    path: string,
    options: YasNativeFsSyncOptions = {},
  ): Promise<YasNativeFsSyncHandle> {
    this.assertOpen();
    validateSyncOptions(options);
    const generation = this.generation;
    const source = this.source(path, options);
    const root = await this.clientForUse().open({
      flags: options.crossFilesystem ? g.YAS_FS_OPEN_CROSS_FILESYSTEM : 0,
      source,
    });
    if (this.disposed || generation !== this.generation) {
      await root.close().catch(() => undefined);
      throw new YasProtocolError(
        "native Workspace FS changed while OPEN was pending",
      );
    }
    const handle = new NativeFsHandle(this, root, options, this.hashBytes);
    this.handles.add(handle);
    try {
      await handle.start(watchOptions(options));
      handle.releaseCallbacks();
      return handle;
    } catch (error) {
      this.handles.delete(handle);
      await root.close().catch(() => undefined);
      throw error;
    }
  }

  async searchFiles(
    rootPath: string,
    query: string,
    limit = 50,
  ): Promise<string[]> {
    if (!Number.isInteger(limit) || limit < 0)
      throw new YasProtocolError("FS search limit is invalid");
    if (limit === 0) return [];
    const root = await this.openPlatformRoot(rootPath);
    try {
      return (
        await this.collectPaths(root, query ? "search" : "index", limit, query)
      ).paths;
    } finally {
      await root.close().catch(() => undefined);
    }
  }

  /**
   * Read a batch of files in one round trip, by absolute path.
   *
   * Each group is one query against one root, so a caller batches by what it
   * wants answered together. That root is the filesystem root rather than the
   * paths' common directory: symlinks are followed, and the server rejects a
   * target that leaves the root — which on a store-based system is every icon
   * under `/run/current-system`, each one a link into `/nix/store`.
   *
   * A file that cannot be read comes back as a record with its status and no
   * bytes, in the caller's order: one unreadable path must not cost the batch.
   */
  async readFiles(
    groups: readonly (readonly string[])[],
    options: { flags?: number; maxBytes?: number } = {},
  ): Promise<{ status: number; path: string; content: Uint8Array }[]> {
    const out: { status: number; path: string; content: Uint8Array }[] = [];
    for (const group of groups) {
      const paths = group.filter((path) => path.startsWith("/"));
      if (paths.length === 0) continue;
      const root = await this.openPlatformRoot("/");
      const relative = paths.map((path) => path.slice(1));
      const answered = new Map<number, { status: number; bytes: Uint8Array }>();
      try {
        const page = await root.read(
          relative.map((path) => ({
            kind: g.YAS_FS_READ_CONTENT,
            flags: options.flags ?? 0,
            path: wirePath(path),
          })),
          BigInt(options.maxBytes ?? DEFAULT_QUERY_CREDIT),
        );
        for (const record of (await page.records()).map(decodeFsQueryRecord)) {
          if (record.kind !== "read") continue;
          answered.set(record.value.questionIndex, {
            status: record.value.status,
            bytes: record.value.content,
          });
        }
      } finally {
        await root.close().catch(() => undefined);
      }
      for (const [index, path] of paths.entries()) {
        const answer = answered.get(index);
        out.push({
          status: answer?.status ?? g.YAS_STATUS_UNAVAILABLE,
          path,
          content: answer?.bytes ?? new Uint8Array(),
        });
      }
    }
    return out;
  }

  async indexFiles(rootPath: string): Promise<FsFileIndex> {
    const root = await this.openPlatformRoot(rootPath);
    try {
      return await this.collectPaths(root, "index", MAX_INDEX_PATHS);
    } finally {
      await root.close().catch(() => undefined);
    }
  }

  async grep(
    rootPath: string,
    query: string,
    options: FsGrepOptions = {},
  ): Promise<FsGrepResult> {
    const root = await this.openPlatformRoot(rootPath);
    const files: FsGrepResult["files"] = [];
    let cursor = new Uint8Array();
    let truncated = false;
    try {
      do {
        const page = await root.grep(
          {
            flags:
              (options.caseSensitive ? g.YAS_FS_GREP_CASE_SENSITIVE : 0) |
              (options.regex ? g.YAS_FS_GREP_REGEX : 0) |
              (options.noIgnore ? g.YAS_FS_GREP_INCLUDE_IGNORED : 0) |
              (options.word ? g.YAS_FS_GREP_WORD : 0),
            maxResults: boundedU16(options.maxMatches ?? 0),
            maxPerFile: boundedU16(options.maxPerFile ?? 0),
            query: encoder.encode(query),
            cursor,
            extensions: [],
          },
          DEFAULT_QUERY_CREDIT,
        );
        appendGrep(files, (await page.records()).map(decodeFsQueryRecord));
        truncated ||= Boolean(page.flags & g.YAS_FS_PAGE_TRUNCATED);
        cursor = new Uint8Array(page.nextCursor);
      } while (cursor.length !== 0);
      return { files, truncated };
    } finally {
      await root.close().catch(() => undefined);
    }
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.generation++;
    this.removeInvalidation();
    for (const handle of [...this.handles]) handle.stop();
    this.handles.clear();
    const client = this.client as { dispose?: () => void } | null;
    client?.dispose?.();
    this.client = null;
  }

  forget(handle: NativeFsHandle): void {
    this.handles.delete(handle);
  }

  private source(
    path: string,
    options: YasNativeFsSyncOptions,
  ): YasFsRootSource {
    if (options.staging) return { kind: "staging" };
    if (options.fromSessionId !== undefined) {
      const terminalHandle = this.options.terminalHandle(options.fromSessionId);
      if (terminalHandle === undefined)
        throw new YasProtocolError("FS source terminal is no longer present");
      return {
        kind: "terminal-cwd",
        terminalHandle,
        suffix: wirePath(path),
      };
    }
    return { kind: "platform-path", path: encoder.encode(path) };
  }

  private async openPlatformRoot(path: string): Promise<YasFsRoot> {
    this.assertOpen();
    const generation = this.generation;
    const root = await this.clientForUse().open({
      flags: 0,
      source: { kind: "platform-path", path: encoder.encode(path) },
    });
    if (this.disposed || generation !== this.generation) {
      await root.close().catch(() => undefined);
      throw new YasProtocolError(
        "native Workspace FS changed while OPEN was pending",
      );
    }
    return root;
  }

  private clientForUse(): Pick<YasFsClient, "open"> {
    return (this.client ??= new YasFsClient(this.connection));
  }

  private async collectPaths(
    root: YasFsRoot,
    kind: "search" | "index",
    maximum: number,
    query = "",
  ): Promise<FsFileIndex> {
    const paths: string[] = [];
    let cursor = new Uint8Array();
    let truncated = false;
    do {
      const left = maximum - paths.length;
      if (left <= 0) {
        truncated = true;
        break;
      }
      const page =
        kind === "search"
          ? await root.search(
              {
                flags: 0,
                maxResults: boundedU16(left),
                query: encoder.encode(query),
                cursor,
                extensions: [],
              },
              DEFAULT_QUERY_CREDIT,
            )
          : await root.index(
              {
                flags: g.YAS_FS_INDEX_INCLUDE_FILES,
                maxResults: boundedU16(left),
                cursor,
                extensions: [],
              },
              DEFAULT_QUERY_CREDIT,
            );
      for (const typed of await page.records()) {
        const record = decodeFsQueryRecord(typed);
        if (record.kind === "unknown") continue;
        if (record.kind !== "path")
          throw new YasProtocolError(`FS ${kind} returned a non-path record`);
        if (!(record.value.flags & g.YAS_FS_QUERY_PATH_DIRECTORY))
          paths.push(pathString(record.value.path));
        if (paths.length >= maximum) break;
      }
      truncated ||= Boolean(page.flags & g.YAS_FS_PAGE_TRUNCATED);
      cursor = new Uint8Array(page.nextCursor);
    } while (cursor.length !== 0);
    return { paths, truncated };
  }

  private assertOpen(): void {
    if (this.disposed)
      throw new YasProtocolError("native Workspace FS is closed");
  }
}

class NativeFsHandle implements YasNativeFsSyncHandle {
  private readonly notifier = new Notifier();
  private readonly lastWritten = new Map<string, Uint8Array>();
  private readonly pendingOperations = new Map<string, string>();
  private removeBatches: (() => void) | null = null;
  private staging: Map<string, YasNativeFsNode> | null = null;
  private callbacksHeld = true;
  private heldCallbacks: Array<() => void> = [];
  private stopped = false;
  private current = new Map<string, YasNativeFsNode>();

  constructor(
    private readonly owner: YasNativeWorkspaceFs,
    private readonly native: YasFsRoot,
    private readonly options: YasNativeFsSyncOptions,
    private readonly hashBytes: (
      bytes: Uint8Array,
    ) => Uint8Array | Promise<Uint8Array>,
  ) {}

  get rootHandle(): bigint {
    return this.native.handle;
  }

  get root(): string {
    return decoder.decode(this.native.opened.canonicalPath);
  }

  get live(): ReadonlyMap<string, YasNativeFsNode> {
    return this.current;
  }

  get revision(): number {
    return this.notifier.revision;
  }

  subscribe = (listener: () => void): (() => void) =>
    this.notifier.subscribe(listener);

  async start(options: ReturnType<typeof watchOptions>): Promise<void> {
    this.removeBatches = this.native.catalog.subscribeBatches((batch) =>
      this.applyBatch(batch),
    );
    await this.native.catalog.watch(options);
  }

  releaseCallbacks(): void {
    queueMicrotask(() => {
      if (this.stopped) return;
      this.callbacksHeld = false;
      for (const callback of this.heldCallbacks.splice(0)) {
        if (this.stopped) break;
        callback();
      }
    });
  }

  async fetch(path: string): Promise<Uint8Array> {
    this.assertOpen();
    const node = this.current.get(path);
    const content = await this.native.fetch(wirePath(path), {
      expectedHash: node && !isZeroHash(node.hash) ? node.hash : undefined,
    });
    return content.bytes();
  }

  writeFile(
    path: string,
    data: Uint8Array,
    options: YasNativeFsWriteOptions = {},
  ): Promise<YasNativeFsWriteResult> {
    return this.uploadBytes(path, new Uint8Array(data), options);
  }

  async upload(
    path: string,
    source: Uint8Array | Blob,
    options: YasNativeFsUploadOptions = {},
  ): Promise<YasNativeFsUploadResult> {
    if (options.signal?.aborted) throw new YasProtocolError("Upload aborted");
    const bytes =
      source instanceof Blob
        ? new Uint8Array(await source.arrayBuffer())
        : new Uint8Array(source);
    return this.uploadBytes(path, bytes, options);
  }

  mkdir(
    path: string,
    options: { mode?: number; createParents?: boolean } = {},
  ): Promise<YasNativeFsWriteResult> {
    return this.applyOne(
      {
        kind: "mkdir",
        path: wirePath(path),
        precondition: { kind: "absent" },
        createParents: options.createParents,
        mode: options.mode ?? 0,
      },
      path,
    );
  }

  async remove(
    path: string,
    options: { ifHash?: Uint8Array } = {},
  ): Promise<void> {
    await this.applyOne(
      {
        kind: "remove",
        path: wirePath(path),
        precondition: options.ifHash
          ? { kind: "hash", contentHash: exactHash(options.ifHash) }
          : { kind: "any" },
        flags: g.YAS_FS_REMOVE_RECURSIVE,
      },
      path,
    );
  }

  async rename(
    from: string,
    to: string,
    options: { createParents?: boolean } = {},
  ): Promise<void> {
    await this.applyOne(
      {
        kind: "rename",
        from: wirePath(from),
        to: wirePath(to),
        precondition: { kind: "any" },
        createParents: options.createParents,
      },
      to,
    );
  }

  symlink(
    target: string,
    path: string,
    options: YasNativeFsLinkOptions = {},
  ): Promise<YasNativeFsWriteResult> {
    return this.applyOne(
      {
        kind: "symlink",
        path: wirePath(path),
        target: encoder.encode(target),
        precondition: linkPrecondition(options),
        createParents: options.createParents,
      },
      path,
    );
  }

  hardlink(
    source: string,
    path: string,
    options: YasNativeFsLinkOptions = {},
  ): Promise<YasNativeFsWriteResult> {
    return this.applyOne(
      {
        kind: "hardlink",
        source: wirePath(source),
        target: wirePath(path),
        precondition: linkPrecondition(options),
        createParents: options.createParents,
      },
      path,
    );
  }

  lastWrittenHash(path: string): Uint8Array | undefined {
    const value = this.lastWritten.get(path);
    return value && new Uint8Array(value);
  }

  stop(): void {
    if (this.stopped) return;
    this.stopped = true;
    this.finishLocal();
    void this.native
      .close()
      .catch(() => undefined)
      .finally(() => {
        invokeLifecycleCallback(() =>
          this.options.onClosed?.(FS_CLOSED_CLIENT_REQUEST),
        );
        invokeLifecycleCallback(() => this.notifier.emit());
      });
  }

  invalidate(): void {
    if (this.stopped) return;
    this.stopped = true;
    this.finishLocal();
    invokeLifecycleCallback(() =>
      this.options.onClosed?.(FS_CLOSED_CONNECTION_LOST),
    );
    invokeLifecycleCallback(() => this.notifier.emit());
  }

  private finishLocal(): void {
    this.owner.forget(this);
    this.removeBatches?.();
    this.removeBatches = null;
    this.current.clear();
    this.staging?.clear();
    this.staging = null;
    this.pendingOperations.clear();
    this.lastWritten.clear();
    this.callbacksHeld = false;
    this.heldCallbacks.length = 0;
  }

  private applyBatch(batch: YasStateBatch): void {
    if (this.stopped) return;
    if (batch.phase === YAS_STATE_RESET) {
      this.staging = new Map();
      this.dispatch(() => invokeLifecycleCallback(this.options.onReset));
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_BEGIN) {
      this.staging = new Map();
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_RECORDS) {
      if (!this.staging)
        throw new YasProtocolError("FS snapshot records have no begin");
      this.applyRecords(this.staging, batch, null);
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_END) {
      if (!this.staging)
        throw new YasProtocolError("FS snapshot end has no begin");
      this.applyRecords(this.staging, batch, null);
      this.current = this.staging;
      this.staging = null;
      const records = [...this.current].map(([path, node]) =>
        recordFromNode(path, node),
      );
      this.dispatch(() => {
        for (const record of records) {
          if (this.stopped) return;
          this.deliver(record);
        }
        if (this.stopped) return;
        invokeLifecycleCallback(this.options.onSync);
        if (this.stopped) return;
        invokeLifecycleCallback(this.options.onUpdate);
        if (this.stopped) return;
        invokeLifecycleCallback(() => this.notifier.emit());
      });
      return;
    }
    if (batch.phase === YAS_STATE_DELTA) {
      const records: YasNativeFsRecord[] = [];
      this.applyRecords(this.current, batch, records);
      this.dispatch(() => {
        for (const record of records) {
          if (this.stopped) return;
          this.deliver(record);
        }
        if (this.stopped) return;
        invokeLifecycleCallback(this.options.onUpdate);
        if (this.stopped) return;
        invokeLifecycleCallback(() => this.notifier.emit());
      });
    }
  }

  private applyRecords(
    target: Map<string, YasNativeFsNode>,
    batch: YasStateBatch,
    output: YasNativeFsRecord[] | null,
  ): void {
    for (const record of batch.records) {
      if (record.kind === YAS_STATE_ADD || record.kind === YAS_STATE_REPLACE) {
        const entry = decodeFsEntry(record.body);
        const converted = convertEntry(entry);
        target.set(converted.path, converted.node);
        this.observeOperation(entry, converted.path, converted.node.hash);
        output?.push(recordFromNode(converted.path, converted.node));
      } else if (record.kind === YAS_STATE_PATCH) {
        const patch = decodeFsEntryPatch(record.body);
        const converted = convertEntry(patch.replacement);
        target.set(converted.path, converted.node);
        this.observeOperation(
          patch.replacement,
          converted.path,
          converted.node.hash,
        );
        output?.push(recordFromNode(converted.path, converted.node));
      } else if (record.kind === YAS_STATE_REMOVE) {
        const removed = decodeFsRemoveRecord(record.body);
        const path = pathString(removed.path);
        removeSubtree(target, path);
        this.consumeOperation(removed.operationId, path);
        output?.push({ kind: "delete", path });
      } else if (record.kind === g.YAS_FS_RECORD_MOVE) {
        const moved = decodeFsMove(record.body);
        const from = pathString(moved.from);
        const to = pathString(moved.to);
        moveSubtree(target, from, to);
        this.consumeOperation(moved.operationId, to);
        output?.push({ kind: "move", from, to });
      }
    }
  }

  private observeOperation(
    entry: YasFsEntryRecord,
    path: string,
    hash: Uint8Array,
  ): void {
    const operation = entry.extensions.find(
      (extension) => extension.tag === g.YAS_FS_ENTRY_OPERATION_ID_EXTENSION,
    );
    if (!operation) return;
    const key = hex(operation.value);
    if (this.pendingOperations.get(key) !== path) return;
    this.pendingOperations.delete(key);
    this.rememberLastWritten(path, hash);
  }

  private consumeOperation(
    operationId: Uint8Array | undefined,
    path: string,
  ): void {
    if (!operationId) return;
    const key = hex(operationId);
    if (this.pendingOperations.get(key) === path)
      this.pendingOperations.delete(key);
  }

  private deliver(record: YasNativeFsRecord): void {
    invokeLifecycleCallback(() => this.options.onRecord?.(record));
    if (this.stopped) return;
    if (record.kind !== "upsert") return;
    const own = this.lastWritten.get(record.path);
    if (own && hashesEqual(own, record.hash))
      this.lastWritten.delete(record.path);
  }

  private dispatch(callback: () => void): void {
    if (this.callbacksHeld) this.heldCallbacks.push(callback);
    else callback();
  }

  private async uploadBytes(
    path: string,
    bytes: Uint8Array,
    options: YasNativeFsUploadOptions,
  ): Promise<YasNativeFsUploadResult> {
    this.assertOpen();
    if (options.signal?.aborted) throw new YasProtocolError("Upload aborted");
    const operationId = randomOperationId();
    const operationKey = hex(operationId);
    this.reservePendingOperation(operationKey, path);
    let contentHash: Uint8Array;
    try {
      contentHash = exactHash(await this.hashBytes(bytes));
    } catch (error) {
      this.pendingOperations.delete(operationKey);
      throw error;
    }
    let staged;
    try {
      staged = await this.native.stageWrite({
        path: wirePath(path),
        precondition: writePrecondition(options),
        flags: options.createParents ? g.YAS_FS_STAGE_CREATE_PARENTS : 0,
        mode: options.mode ?? 0,
        byteLength: BigInt(bytes.length),
        contentHash,
        initialReceiveCredit:
          bytes.length === 0
            ? 0n
            : BigInt(Math.min(bytes.length, Number(DEFAULT_UPLOAD_CREDIT))),
        extensions: [],
      });
    } catch (error) {
      this.pendingOperations.delete(operationKey);
      throw mutationError(error);
    }
    const chunkSize = options.chunkSize ?? DEFAULT_UPLOAD_CHUNK;
    if (!Number.isInteger(chunkSize) || chunkSize <= 0) {
      this.pendingOperations.delete(operationKey);
      staged.transfer.reset();
      throw new YasProtocolError("chunkSize must be a positive integer");
    }
    const abort = (): void => staged.transfer.reset();
    options.signal?.addEventListener("abort", abort, { once: true });
    try {
      for (let offset = 0; offset < bytes.length; offset += chunkSize) {
        if (options.signal?.aborted)
          throw new YasProtocolError("Upload aborted");
        const end = Math.min(bytes.length, offset + chunkSize);
        await staged.transfer.write(bytes.subarray(offset, end));
        options.onProgress?.(end, bytes.length);
      }
      staged.transfer.closeWrite();
      await staged.transfer.closed;
    } catch (error) {
      this.pendingOperations.delete(operationKey);
      staged.transfer.reset();
      throw error;
    } finally {
      options.signal?.removeEventListener("abort", abort);
    }
    try {
      const result = await this.native.commit(
        staged.stagingHandle,
        operationId,
        options.durable
          ? g.YAS_FS_COMMIT_SYNC_DATA | g.YAS_FS_COMMIT_SYNC_DIRECTORY
          : 0,
      );
      const hash = exactHash(result.contentHash);
      if (this.pendingOperations.has(operationKey))
        this.rememberLastWritten(path, hash);
      return {
        hash: new Uint8Array(hash),
        mtime: Number(result.modifiedUnixNs),
        mtimeNs: result.modifiedUnixNs,
      };
    } catch (error) {
      this.pendingOperations.delete(operationKey);
      throw mutationError(error);
    }
  }

  private async applyOne(
    item: YasFsApplyItem,
    resultPath: string,
  ): Promise<YasNativeFsWriteResult> {
    this.assertOpen();
    const operationId = randomOperationId();
    const operationKey = hex(operationId);
    this.reservePendingOperation(operationKey, resultPath);
    try {
      const result = await this.native.apply({
        operationId,
        flags: g.YAS_FS_APPLY_ALL_OR_NONE,
        items: [item],
        extensions: [],
      });
      const applied = result.items[0];
      if (!applied)
        throw new YasProtocolError("FS APPLY omitted its item result");
      checkApplyResult(applied);
      const hash = applied.contentHash
        ? exactHash(applied.contentHash)
        : new Uint8Array(ZERO_HASH);
      if (!isZeroHash(hash) && this.pendingOperations.has(operationKey))
        this.rememberLastWritten(resultPath, hash);
      return { hash: new Uint8Array(hash), mtimeNs: applied.modifiedUnixNs };
    } catch (error) {
      this.pendingOperations.delete(operationKey);
      throw mutationError(error);
    }
  }

  private reservePendingOperation(operationKey: string, path: string): void {
    if (this.pendingOperations.has(operationKey))
      throw new YasProtocolError("FS operation ID source collided");
    if (this.pendingOperations.size >= MAX_PENDING_MUTATIONS)
      throw new YasProtocolError("FS pending-mutation budget is exhausted");
    this.pendingOperations.set(operationKey, path);
  }

  private rememberLastWritten(path: string, hash: Uint8Array): void {
    this.lastWritten.delete(path);
    this.lastWritten.set(path, exactHash(hash));
    while (this.lastWritten.size > MAX_REMEMBERED_WRITES) {
      const oldest = this.lastWritten.keys().next().value;
      if (oldest === undefined) break;
      this.lastWritten.delete(oldest);
    }
  }

  private assertOpen(): void {
    if (this.stopped) throw new YasProtocolError("FS root is closed");
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
    else console.error("YAS FS lifecycle callback failed", error);
  } catch {
    // Cleanup must not depend on host error reporting.
  }
}

function watchOptions(options: YasNativeFsSyncOptions) {
  let flags = options.content ? g.YAS_FS_WATCH_CONTENT : 0;
  if (!options.single) {
    if (options.recursive !== false) flags |= g.YAS_FS_WATCH_RECURSIVE;
    flags |= g.YAS_FS_WATCH_INCLUDE_HIDDEN;
    if (options.ignore || options.gitignore) flags |= g.YAS_FS_WATCH_GITIGNORE;
    if (options.ignore || options.dotIgnore) flags |= g.YAS_FS_WATCH_DOT_IGNORE;
    if (options.ignore || options.excludeGit)
      flags |= g.YAS_FS_WATCH_EXCLUDE_GIT;
  }
  return {
    flags,
    settleMs: options.latencyMs ?? 0,
    inlineMax:
      options.inlineMax === undefined || options.inlineMax === 0
        ? g.YAS_FS_MAX_INLINE_BYTES
        : Math.min(options.inlineMax, g.YAS_FS_MAX_INLINE_BYTES),
    ignorePatterns: (options.exclude ?? []).join("\n"),
  };
}

function validateSyncOptions(options: YasNativeFsSyncOptions): void {
  if (options.single && options.recursive)
    throw new YasProtocolError("a single-file sync cannot be recursive");
  if (options.staging && options.fromSessionId)
    throw new YasProtocolError("a staging sync cannot use a terminal cwd");
  const settle = options.latencyMs ?? 0;
  if (
    !Number.isInteger(settle) ||
    settle < 0 ||
    settle > g.YAS_FS_MAX_WATCH_SETTLE_MS
  )
    throw new YasProtocolError(
      "FS settle delay is outside the negotiated limit",
    );
}

function writePrecondition(
  options: YasNativeFsWriteOptions,
): YasFsPrecondition {
  const selectors =
    Number(options.force) +
    Number(options.create) +
    Number(options.ifHash !== undefined);
  if (selectors > 1)
    throw new YasProtocolError("FS write preconditions are mutually exclusive");
  if (options.force) return { kind: "any" };
  if (options.create) return { kind: "absent" };
  if (options.ifHash)
    return { kind: "hash", contentHash: exactHash(options.ifHash) };
  return { kind: "any" };
}

function linkPrecondition(options: YasNativeFsLinkOptions): YasFsPrecondition {
  if (options.force) return { kind: "any" };
  if (options.ifHash)
    return { kind: "hash", contentHash: exactHash(options.ifHash) };
  return { kind: "absent" };
}

function checkApplyResult(result: YasFsApplyItemResult): void {
  if (result.status === g.YAS_STATUS_OK) return;
  if (result.status === g.YAS_STATUS_CONFLICT)
    throw new YasNativeFsConflictError(result.contentHash ?? ZERO_HASH);
  if (result.status === g.YAS_STATUS_IO)
    throw new YasNativeFsPermissionError(result.detail);
  throw new YasResultError(
    result.status,
    encoder.encode(result.detail),
    "FS APPLY item failed",
  );
}

function mutationError(error: unknown): Error {
  if (error instanceof YasNativeFsConflictError) return error;
  if (
    error instanceof YasResultError &&
    error.status === g.YAS_STATUS_CONFLICT
  ) {
    const detail = decodeFsConflictDetail(error.detail);
    return new YasNativeFsConflictError(detail.currentHash ?? ZERO_HASH);
  }
  return error instanceof Error ? error : new Error(String(error));
}

function convertEntry(entry: YasFsEntryRecord): {
  path: string;
  node: YasNativeFsNode;
} {
  let entryFlags: number;
  let size = 0;
  let hash: Uint8Array = new Uint8Array(ZERO_HASH);
  let content: Uint8Array | null = null;
  if (entry.body.kind === "file") {
    entryFlags = FS_ENTRY_FILE;
    size = safeLength(entry.body.byteLength);
    hash = exactHash(entry.body.contentHash);
    content = entry.body.inlineContent
      ? new Uint8Array(entry.body.inlineContent)
      : null;
    if (entry.body.inlineContent === undefined)
      entryFlags |= FS_ENTRY_NO_CONTENT;
  } else if (entry.body.kind === "directory") {
    entryFlags = FS_ENTRY_DIR;
  } else {
    entryFlags = FS_ENTRY_SYMLINK;
    size = entry.body.target.length;
    hash = exactHash(entry.body.contentHash);
    content = new Uint8Array(entry.body.target);
  }
  if (entry.flags & g.YAS_FS_ENTRY_UNREADABLE)
    entryFlags |= FS_ENTRY_UNREADABLE;
  if (entry.flags & g.YAS_FS_ENTRY_UNSTABLE) entryFlags |= FS_ENTRY_UNSTABLE;
  if (entry.flags & g.YAS_FS_ENTRY_SYMLINK_DIRECTORY)
    entryFlags |= FS_ENTRY_LINK_DIR;
  if (entry.flags & g.YAS_FS_ENTRY_DIRECTORY_FILTERED)
    entryFlags |= FS_ENTRY_FILTERED;
  if (entryFlags & (FS_ENTRY_UNREADABLE | FS_ENTRY_UNSTABLE)) content = null;
  return {
    path: pathString(entry.path),
    node: {
      entryFlags,
      size,
      mtimeNs: entry.modifiedUnixNs,
      mode: entry.mode,
      hash,
      content,
    },
  };
}

function recordFromNode(
  path: string,
  node: YasNativeFsNode,
): YasNativeFsRecord {
  return {
    kind: "upsert",
    path,
    entryFlags: node.entryFlags,
    size: node.size,
    mtimeNs: node.mtimeNs,
    mode: node.mode,
    hash: new Uint8Array(node.hash),
    content:
      node.content === null
        ? { kind: "none" }
        : { kind: "full", data: new Uint8Array(node.content) },
  };
}

function removeSubtree(
  target: Map<string, YasNativeFsNode>,
  root: string,
): void {
  for (const path of [...target.keys()])
    if (root === "" || path === root || path.startsWith(`${root}/`))
      target.delete(path);
}

function moveSubtree(
  target: Map<string, YasNativeFsNode>,
  from: string,
  to: string,
): void {
  const moved = [...target].filter(
    ([path]) => path === from || from === "" || path.startsWith(`${from}/`),
  );
  for (const [path] of moved) target.delete(path);
  for (const [path, node] of moved) {
    const suffix =
      path === from ? "" : path.slice(from.length + (from ? 1 : 0));
    target.set(suffix ? `${to}${to ? "/" : ""}${suffix}` : to, node);
  }
}

function appendGrep(
  output: FsGrepResult["files"],
  records: readonly YasFsQueryRecord[],
): void {
  const files = new Map<number, FsGrepResult["files"][number]>();
  for (const record of records) {
    if (record.kind === "unknown") continue;
    if (record.kind === "grep-file") {
      const file = {
        path: pathString(record.value.path),
        ignored: Boolean(record.value.flags & g.YAS_FS_QUERY_GREP_FILE_IGNORED),
        matches: [],
      } satisfies FsGrepResult["files"][number];
      files.set(record.value.fileIndex, file);
      output.push(file);
    } else if (record.kind === "grep-match") {
      const file = files.get(record.value.fileIndex);
      if (!file)
        throw new YasProtocolError("FS GREP match names an unknown file");
      file.matches.push({
        line: record.value.line,
        col: record.value.column,
        endLine: record.value.endLine,
        endCol: record.value.endColumn,
        text: record.value.text,
      });
    } else {
      throw new YasProtocolError("FS GREP returned the wrong record kind");
    }
  }
}

function wirePath(path: string): YasFsPath {
  if (path === "" || path === ".") return { components: [] };
  const normalized = path.replace(/\\/g, "/");
  if (normalized.startsWith("/"))
    throw new YasProtocolError("FS mirror paths must be root-relative");
  return {
    components: normalized
      .split("/")
      .filter((component) => component.length !== 0 && component !== ".")
      .map((component) => {
        if (component === "..")
          throw new YasProtocolError("FS mirror path traverses above its root");
        return encoder.encode(component);
      }),
  };
}

function pathString(path: YasFsPath): string {
  return path.components
    .map((component) => decoder.decode(component))
    .join("/");
}

function exactHash(hash: Uint8Array): Uint8Array {
  if (hash.length !== 32)
    throw new YasProtocolError("FS content hash is not 32 bytes");
  return new Uint8Array(hash);
}

export function yasNativeFsHashesEqual(
  left: Uint8Array | null | undefined,
  right: Uint8Array | null | undefined,
): boolean {
  return hashesEqual(left, right);
}

function hashesEqual(
  left: Uint8Array | null | undefined,
  right: Uint8Array | null | undefined,
): boolean {
  if (left === right) return true;
  if (!left || !right || left.length !== 32 || right.length !== 32)
    return false;
  let difference = 0;
  for (let index = 0; index < 32; index++)
    difference |= left[index]! ^ right[index]!;
  return difference === 0;
}

function isZeroHash(hash: Uint8Array): boolean {
  return hash.length === 32 && hash.every((byte) => byte === 0);
}

function randomOperationId(): Uint8Array {
  const value = globalThis.crypto.getRandomValues(new Uint8Array(16));
  if (value.every((byte) => byte === 0)) value[0] = 1;
  return value;
}

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(
    "",
  );
}

function boundedU16(value: number): number {
  if (!Number.isFinite(value) || value < 0)
    throw new YasProtocolError("FS query limit is invalid");
  return Math.min(0xffff, Math.trunc(value));
}

function safeLength(value: bigint): number {
  if (value > BigInt(Number.MAX_SAFE_INTEGER))
    throw new YasProtocolError(
      "FS file size exceeds browser integer precision",
    );
  return Number(value);
}

async function defaultBlake3(bytes: Uint8Array): Promise<Uint8Array> {
  const { blake3_hash } = await import("@yas-run/browser");
  return blake3_hash(bytes);
}
