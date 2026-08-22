/** Workspace-facing language intelligence backed directly by typed YAS LSP. */

import {
  LSP_CLOSED_CLIENT_REQUEST,
  LSP_CLOSED_CONNECTION_LOST,
  LSP_COMPLETION_DEPRECATED,
  LSP_COMPLETION_PRESELECT,
  LSP_COMPLETION_SNIPPET,
  LSP_STATUS_BUDGET,
  LSP_STATUS_CANCELLED,
  LSP_STATUS_INVALID,
  LSP_STATUS_NOT_FOUND,
  LSP_STATUS_OK,
  LSP_STATUS_OTHER,
  LSP_STATUS_WARMING,
  LSP_STATUS_WRONG_TYPE,
  type LspOpenOptions,
} from "../lspModel";
import { Notifier } from "../reactive";
import type { SessionId } from "../types";
import * as g from "./generated";
import type { YasFsPath } from "./fs";
import {
  YasLspClient,
  decodeLspNoBackendDetail,
  type YasLspBufferIdentity,
  type YasLspDocumentTarget,
  type YasLspQueryBody,
  type YasLspQueryRecord,
  type YasLspSnapshot,
  type YasLspWorkspace,
} from "./lsp";
import type { YasConnection } from "./session";
import { YasProtocolError } from "./wire";

const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });
const DEFAULT_QUERY_CREDIT = BigInt(g.YAS_LSP_MAX_QUERY_BYTES);
const DEFAULT_UPLOAD_CREDIT = 1024n * 1024n;
// A slow upload may retain the bytes currently on the wire and one coalesced
// replacement. Keep that aggregate tied to the protocol's per-buffer ceiling;
// callers retry after the in-flight upload settles rather than growing an
// unbounded browser-side edit log.
const MAX_RETAINED_BUFFER_BYTES = 2 * g.YAS_LSP_MAX_BUFFER_BYTES;
const MAX_TRACKED_BUFFER_PATHS = g.YAS_LSP_MAX_BUFFERS_PER_WORKSPACE;

type BufferMutation =
  | {
      kind: "put";
      path: YasFsPath;
      content: Uint8Array;
      retainedBytes: number;
    }
  | { kind: "release"; path: YasFsPath; retainedBytes: 0 };

interface BufferQueue {
  active: BufferMutation | null;
  pending: BufferMutation | null;
  running: Promise<void>;
}

export interface YasNativeLspServerState {
  serverHandle: bigint;
  generation: bigint;
  serverRevision: bigint;
  phase: number;
  progressPct: number;
  caps: bigint;
  epoch: number;
  refusedEdits: number;
  rss: bigint;
  id: string;
  language: string;
  profile: string;
  msg: string;
}

export interface YasNativeLspState {
  readonly servers: ReadonlyMap<bigint, YasNativeLspServerState>;
  readonly revision: bigint;
}

export interface YasNativeLspDiagnostic {
  diagnosticId: bigint;
  severity: number;
  flags: number;
  line: number;
  col: number;
  endLine: number;
  endCol: number;
  code: string;
  source: string;
  msg: string;
}

export interface YasNativeLspFileDiags {
  /** Exact 32-byte document content hash. */
  hash: Uint8Array;
  documentRevision: bigint;
  diagnosticsRevision: bigint;
  diags: readonly YasNativeLspDiagnostic[];
}

export interface YasNativeLspDiagnostics {
  readonly files: ReadonlyMap<string, YasNativeLspFileDiags>;
  readonly revision: bigint;
}

export type YasNativeLspResultRecord =
  | {
      kind: "location";
      flags: number;
      hash: Uint8Array;
      line: number;
      col: number;
      endLine: number;
      endCol: number;
      path: string;
    }
  | { kind: "markup"; format: number; text: string }
  | {
      kind: "symbol";
      symKind: number;
      flags: number;
      depth: number;
      line: number;
      col: number;
      endLine: number;
      endCol: number;
      name: string;
      path: string;
    }
  | {
      kind: "completion";
      itemKind: number;
      flags: number;
      line: number;
      col: number;
      endLine: number;
      endCol: number;
      label: string;
      insert: string;
      detail: string;
    }
  | {
      kind: "edit";
      flags: number;
      hash: Uint8Array;
      line: number;
      col: number;
      endLine: number;
      endCol: number;
      newText: string;
      path: string;
    }
  | {
      kind: "signature";
      flags: number;
      activeParam: number;
      paramStart: number;
      paramEnd: number;
      label: string;
      doc: string;
    }
  | {
      kind: "action";
      actionKind: string;
      title: string;
      flags: number;
      disabledReason: string;
      edits: readonly Extract<YasNativeLspResultRecord, { kind: "edit" }>[];
    };

export interface YasNativeLspQueryResult {
  status: number;
  detail: string;
  truncated: boolean;
  incomplete: boolean;
  records: YasNativeLspResultRecord[];
}

export interface YasNativeLspOpenOptions extends Omit<
  LspOpenOptions,
  "onState" | "onDiagnostics"
> {
  onState?: (state: YasNativeLspState, revision: bigint) => void;
  onDiagnostics?: (
    diagnostics: YasNativeLspDiagnostics,
    revision: bigint,
  ) => void;
}

export interface YasNativeLspHandle {
  readonly workspaceHandle: bigint;
  readonly workspaceRevision: bigint;
  readonly root: string;
  readonly state: YasNativeLspState;
  readonly diags: YasNativeLspDiagnostics;
  readonly revision: number;
  subscribe(listener: () => void): () => void;
  definition(
    path: string,
    line: number,
    col: number,
  ): Promise<YasNativeLspQueryResult>;
  references(
    path: string,
    line: number,
    col: number,
    includeDeclaration?: boolean,
  ): Promise<YasNativeLspQueryResult>;
  hover(
    path: string,
    line: number,
    col: number,
  ): Promise<YasNativeLspQueryResult>;
  documentSymbols(path: string): Promise<YasNativeLspQueryResult>;
  workspaceSymbols(query: string): Promise<YasNativeLspQueryResult>;
  rename(
    path: string,
    line: number,
    col: number,
    newName: string,
  ): Promise<YasNativeLspQueryResult>;
  completion(
    path: string,
    line: number,
    col: number,
  ): Promise<YasNativeLspQueryResult>;
  signatureHelp(
    path: string,
    line: number,
    col: number,
  ): Promise<YasNativeLspQueryResult>;
  buffer(path: string, text: Uint8Array): void;
  releaseBuffer(path: string): void;
  close(): void;
}

export interface YasNativeWorkspaceLspOptions {
  terminalHandle(sessionId: SessionId): bigint | undefined;
  client?: Pick<YasLspClient, "open">;
  hashBytes?: (bytes: Uint8Array) => Uint8Array | Promise<Uint8Array>;
  operationId?: () => Uint8Array;
}

export class YasNativeWorkspaceLsp {
  private client: Pick<YasLspClient, "open"> | null;
  private readonly handles = new Set<NativeLspHandle>();
  private readonly hashBytes: NonNullable<
    YasNativeWorkspaceLspOptions["hashBytes"]
  >;
  private readonly operationId: () => Uint8Array;
  private readonly removeInvalidation: () => void;
  private generation = 0;
  private disposed = false;

  constructor(
    readonly connection: YasConnection,
    private readonly options: YasNativeWorkspaceLspOptions,
  ) {
    this.client = options.client ?? null;
    this.hashBytes = options.hashBytes ?? defaultBlake3;
    this.operationId = options.operationId ?? randomOperationId;
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family !== undefined && family !== g.YAS_FAMILY_LSP) return;
      this.generation++;
      for (const handle of [...this.handles]) handle.invalidate();
    });
  }

  async openLsp(
    path: string,
    options: YasNativeLspOpenOptions = {},
  ): Promise<YasNativeLspHandle> {
    this.assertOpen();
    validateOpenOptions(options);
    const generation = this.generation;
    const source =
      options.fromSessionId === undefined
        ? ({ kind: "platform-path", path: encoder.encode(path) } as const)
        : ({
            kind: "terminal-cwd",
            terminalHandle: this.requireTerminal(options.fromSessionId),
            suffix: wirePath(path),
          } as const);
    const native = await this.clientForUse().open({
      source,
      openMode: g.YAS_LSP_OPEN_AUTO_DISCOVER,
      diagnosticsSettleMs: options.diagLatencyMs ?? 0,
      language: "",
      profile: "",
      initializationOptions: new Uint8Array(),
      extensions: [],
    });
    if (this.disposed || generation !== this.generation) {
      await native.close().catch(() => undefined);
      throw new YasProtocolError(
        "native Workspace LSP changed while OPEN was pending",
      );
    }
    if (native.opened.backendCount === 0) {
      const detail =
        decodeLspNoBackendDetail(native.opened.extensions) ??
        "no language server matched this workspace";
      await native.close().catch(() => undefined);
      throw new YasProtocolError(`Open failed: ${detail}`);
    }
    const handle = new NativeLspHandle(
      this,
      native,
      options,
      this.hashBytes,
      this.operationId,
    );
    this.handles.add(handle);
    try {
      await handle.start();
      return handle;
    } catch (error) {
      this.handles.delete(handle);
      await native.close().catch(() => undefined);
      throw error;
    }
  }

  private clientForUse(): Pick<YasLspClient, "open"> {
    return (this.client ??= new YasLspClient(this.connection));
  }

  forget(handle: NativeLspHandle): void {
    this.handles.delete(handle);
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.generation++;
    this.removeInvalidation();
    for (const handle of [...this.handles]) handle.close();
    this.handles.clear();
    const client = this.client as { dispose?: () => void } | null;
    client?.dispose?.();
    this.client = null;
  }

  private requireTerminal(sessionId: SessionId): bigint {
    const handle = this.options.terminalHandle(sessionId);
    if (handle === undefined)
      throw new YasProtocolError("LSP source terminal is no longer present");
    return handle;
  }

  private assertOpen(): void {
    if (this.disposed)
      throw new YasProtocolError("native Workspace LSP is closed");
  }
}

class NativeLspHandle implements YasNativeLspHandle {
  private readonly notifier = new Notifier();
  private readonly buffers = new Map<string, YasLspBufferIdentity>();
  private readonly bufferQueues = new Map<string, BufferQueue>();
  private readonly bufferFailures = new Map<string, Error>();
  private retainedBufferBytes = 0;
  private removeCatalog: (() => void) | null = null;
  private removeClosed: (() => void) | null = null;
  private closed = false;
  private stateValue: YasNativeLspState = { servers: new Map(), revision: 0n };
  private diagnosticsValue: YasNativeLspDiagnostics = {
    files: new Map(),
    revision: 0n,
  };

  constructor(
    private readonly owner: YasNativeWorkspaceLsp,
    private readonly native: YasLspWorkspace,
    private readonly options: YasNativeLspOpenOptions,
    private readonly hashBytes: (
      bytes: Uint8Array,
    ) => Uint8Array | Promise<Uint8Array>,
    private readonly makeOperationId: () => Uint8Array,
  ) {}

  get workspaceHandle(): bigint {
    return this.native.handle;
  }

  get workspaceRevision(): bigint {
    return this.native.opened.workspaceRevision;
  }

  get root(): string {
    return decoder.decode(this.native.opened.canonicalRoot);
  }

  get state(): YasNativeLspState {
    return this.stateValue;
  }

  get diags(): YasNativeLspDiagnostics {
    return this.diagnosticsValue;
  }

  get revision(): number {
    return this.notifier.revision;
  }

  subscribe = this.notifier.subscribe;

  async start(): Promise<void> {
    this.removeClosed = this.native.onClosed((event) => {
      this.finish(
        event.detail === "LSP session invalidated"
          ? LSP_CLOSED_CONNECTION_LOST
          : event.reason,
      );
    });
    if (!this.options.watch && !this.options.diagnostics) return;
    this.removeCatalog = this.native.catalog.subscribe((snapshot) =>
      this.applySnapshot(snapshot),
    );
    await this.native.catalog.watch({
      datasets:
        g.YAS_LSP_WATCH_BACKEND |
        (this.options.diagnostics ? g.YAS_LSP_WATCH_DIAGNOSTICS : 0),
    });
    if (this.closed) await this.native.catalog.unwatch().catch(() => undefined);
  }

  definition(path: string, line: number, col: number) {
    return this.targetQuery(path, (target) => ({
      kind: "definition",
      target,
      position: { line, byteColumn: col },
    }));
  }

  references(
    path: string,
    line: number,
    col: number,
    includeDeclaration = false,
  ) {
    return this.targetQuery(path, (target) => ({
      kind: "references",
      target,
      position: { line, byteColumn: col },
      flags: includeDeclaration ? g.YAS_LSP_REFERENCES_INCLUDE_DECLARATION : 0,
    }));
  }

  hover(path: string, line: number, col: number) {
    return this.targetQuery(path, (target) => ({
      kind: "hover",
      target,
      position: { line, byteColumn: col },
    }));
  }

  documentSymbols(path: string) {
    return this.targetQuery(
      path,
      (target) => ({ kind: "document-symbols", target }),
      path,
    );
  }

  async workspaceSymbols(query: string) {
    await this.waitForAllBuffers();
    return this.query({ kind: "workspace-symbols", query });
  }

  rename(path: string, line: number, col: number, newName: string) {
    return this.targetQuery(path, (target) => ({
      kind: "rename",
      target,
      position: { line, byteColumn: col },
      newName,
    }));
  }

  completion(path: string, line: number, col: number) {
    return this.targetQuery(path, (target) => ({
      kind: "completion",
      target,
      position: { line, byteColumn: col },
      triggerKind: g.YAS_LSP_COMPLETION_TRIGGER_INVOKED,
      trigger: "",
    }));
  }

  signatureHelp(path: string, line: number, col: number) {
    return this.targetQuery(path, (target) => ({
      kind: "signature-help",
      target,
      position: { line, byteColumn: col },
    }));
  }

  buffer(path: string, text: Uint8Array): void {
    this.assertOpen();
    if (text.length > g.YAS_LSP_MAX_BUFFER_BYTES)
      throw new YasProtocolError("LSP buffer exceeds the native size limit");
    const targetPath = wirePath(path);
    const key = pathString(targetPath);
    const content = new Uint8Array(text);
    this.enqueueBuffer(key, {
      kind: "put",
      path: targetPath,
      content,
      retainedBytes: content.length,
    });
  }

  releaseBuffer(path: string): void {
    this.assertOpen();
    const targetPath = wirePath(path);
    const key = pathString(targetPath);
    if (!this.buffers.has(key) && !this.bufferQueues.has(key)) {
      this.bufferFailures.delete(key);
      return;
    }
    this.enqueueBuffer(key, {
      kind: "release",
      path: targetPath,
      retainedBytes: 0,
    });
  }

  close(): void {
    if (this.closed) return;
    this.finish(LSP_CLOSED_CLIENT_REQUEST);
    void this.native.close().catch(() => undefined);
  }

  invalidate(): void {
    if (this.closed) return;
    this.finish(LSP_CLOSED_CONNECTION_LOST);
    void this.native.close().catch(() => undefined);
  }

  private async targetQuery(
    path: string,
    build: (target: YasLspDocumentTarget) => YasLspQueryBody,
    defaultPath = "",
  ): Promise<YasNativeLspQueryResult> {
    const targetPath = wirePath(path);
    const key = pathString(targetPath);
    await this.waitForBuffer(key);
    const identity = this.buffers.get(key);
    const target: YasLspDocumentTarget = identity
      ? {
          path: targetPath,
          documentRevision: identity.bufferRevision,
          contentHash: new Uint8Array(identity.contentHash),
        }
      : {
          path: targetPath,
          documentRevision: 0n,
          contentHash: new Uint8Array(32),
        };
    return this.query(build(target), defaultPath);
  }

  private async query(
    body: YasLspQueryBody,
    defaultPath = "",
  ): Promise<YasNativeLspQueryResult> {
    this.assertOpen();
    const page = await this.native.query(body, {
      maxRecords: g.YAS_LSP_MAX_QUERY_RECORDS,
      initialReceiveCredit: DEFAULT_QUERY_CREDIT,
    });
    const records: YasNativeLspResultRecord[] = [];
    for (const record of await page.records())
      records.push(...queryRecords(record, defaultPath));
    return {
      status: workspaceStatus(page.queryStatus),
      detail: page.detail,
      truncated: Boolean(page.flags & g.YAS_LSP_PAGE_TRUNCATED),
      incomplete: Boolean(page.flags & g.YAS_LSP_PAGE_INCOMPLETE),
      records,
    };
  }

  private async uploadBuffer(
    path: YasFsPath,
    expectedRevision: bigint,
    content: Uint8Array,
  ): Promise<YasLspBufferIdentity> {
    const contentHash = exactHash(await this.hashBytes(content));
    const staged = await this.native.bufferBegin({
      expectedRevision,
      path,
      byteLength: BigInt(content.length),
      contentHash,
      initialSendCredit: BigInt(
        Math.min(content.length, Number(DEFAULT_UPLOAD_CREDIT)),
      ),
      extensions: [],
    });
    try {
      await staged.transfer.write(content);
      staged.transfer.closeWrite();
      await staged.transfer.closed;
    } catch (error) {
      staged.transfer.reset();
      throw error;
    }
    return this.native.bufferCommit({
      stagingHandle: staged.stagingHandle,
      operationId: this.operationId(),
      extensions: [],
    });
  }

  private enqueueBuffer(key: string, mutation: BufferMutation): void {
    let queue = this.bufferQueues.get(key);
    if (
      !queue &&
      !this.buffers.has(key) &&
      this.trackedBufferPathCount() >= MAX_TRACKED_BUFFER_PATHS
    )
      throw new YasProtocolError("LSP buffered-path limit is exhausted");

    const replacedBytes = queue?.pending?.retainedBytes ?? 0;
    const projectedBytes =
      this.retainedBufferBytes - replacedBytes + mutation.retainedBytes;
    if (projectedBytes > MAX_RETAINED_BUFFER_BYTES)
      throw new YasProtocolError("LSP pending-buffer byte budget is exhausted");

    this.retainedBufferBytes = projectedBytes;
    this.bufferFailures.delete(key);
    if (queue) {
      queue.pending = mutation;
      return;
    }

    queue = {
      active: null,
      pending: mutation,
      running: Promise.resolve(),
    };
    this.bufferQueues.set(key, queue);
    queue.running = this.drainBufferQueue(key, queue);
  }

  private async waitForBuffer(key: string): Promise<void> {
    while (true) {
      const queue = this.bufferQueues.get(key);
      if (!queue) break;
      await queue.running;
    }
    const failure = this.bufferFailures.get(key);
    if (failure) throw failure;
  }

  private async waitForAllBuffers(): Promise<void> {
    while (this.bufferQueues.size !== 0)
      await Promise.all(
        [...this.bufferQueues.values()].map((queue) => queue.running),
      );
    const failure = this.bufferFailures.values().next().value;
    if (failure) throw failure;
  }

  private async drainBufferQueue(
    key: string,
    queue: BufferQueue,
  ): Promise<void> {
    try {
      while (!this.closed && queue.pending) {
        const mutation = queue.pending;
        queue.pending = null;
        queue.active = mutation;
        try {
          await this.performBufferMutation(key, mutation);
          this.bufferFailures.delete(key);
        } catch (error) {
          if (mutation.kind === "release") this.bufferFailures.delete(key);
          else this.recordBufferFailure(key, asError(error));
        } finally {
          queue.active = null;
          this.retainedBufferBytes = Math.max(
            0,
            this.retainedBufferBytes - mutation.retainedBytes,
          );
        }
      }
    } finally {
      if (this.bufferQueues.get(key) === queue) this.bufferQueues.delete(key);
    }
  }

  private async performBufferMutation(
    key: string,
    mutation: BufferMutation,
  ): Promise<void> {
    const identity = this.buffers.get(key);
    if (mutation.kind === "release") {
      if (!identity) return;
      await this.native.bufferClose({
        bufferHandle: identity.bufferHandle,
        expectedRevision: identity.bufferRevision,
        operationId: this.operationId(),
        extensions: [],
      });
      this.buffers.delete(key);
      return;
    }

    const expectedRevision = identity?.bufferRevision ?? 0n;
    const updated =
      mutation.content.length <= g.YAS_LSP_MAX_INLINE_BUFFER_BYTES
        ? await this.native.bufferPut({
            operationId: this.operationId(),
            expectedRevision,
            path: mutation.path,
            content: mutation.content,
            extensions: [],
          })
        : await this.uploadBuffer(
            mutation.path,
            expectedRevision,
            mutation.content,
          );
    if (!this.closed) this.buffers.set(key, cloneIdentity(updated));
  }

  private trackedBufferPathCount(): number {
    let count = this.buffers.size;
    for (const key of this.bufferQueues.keys())
      if (!this.buffers.has(key)) count++;
    return count;
  }

  private recordBufferFailure(key: string, error: Error): void {
    this.bufferFailures.delete(key);
    this.bufferFailures.set(key, error);
    while (this.bufferFailures.size > MAX_TRACKED_BUFFER_PATHS) {
      const oldest = this.bufferFailures.keys().next().value;
      if (oldest === undefined) break;
      this.bufferFailures.delete(oldest);
    }
  }

  private applySnapshot(snapshot: YasLspSnapshot): void {
    if (
      snapshot.revision === this.stateValue.revision &&
      snapshot.revision === this.diagnosticsValue.revision
    )
      return;
    if (this.options.watch || this.options.diagnostics) {
      this.stateValue = {
        revision: snapshot.revision,
        servers: new Map(
          snapshot.backends.map((server) => [
            server.serverHandle,
            {
              serverHandle: server.serverHandle,
              generation: server.generation,
              serverRevision: server.serverRevision,
              phase: server.phase,
              progressPct: server.progressPercent,
              caps: server.capabilities,
              epoch: server.epoch,
              refusedEdits: server.refusedEdits,
              rss: server.rssBytes,
              id: server.backendId,
              language: server.language,
              profile: server.profile,
              msg: server.lastMessage,
            },
          ]),
        ),
      };
    }
    if (this.options.diagnostics) {
      this.diagnosticsValue = {
        revision: snapshot.revision,
        files: new Map(
          snapshot.diagnostics
            .filter((file) => file.diagnostics.length !== 0)
            .map((file) => [
              pathString(file.path),
              {
                hash: exactHash(file.contentHash),
                documentRevision: file.documentRevision,
                diagnosticsRevision: file.diagnosticsRevision,
                diags: file.diagnostics.map((diagnostic) => ({
                  diagnosticId: diagnostic.diagnosticId,
                  severity: diagnostic.severity + 1,
                  flags: diagnostic.tags,
                  line: diagnostic.range.start.line,
                  col: diagnostic.range.start.byteColumn,
                  endLine: diagnostic.range.end.line,
                  endCol: diagnostic.range.end.byteColumn,
                  code: diagnostic.code,
                  source: diagnostic.source,
                  msg: diagnostic.message,
                })),
              },
            ]),
        ),
      };
    }
    invokeLifecycleCallback(() => this.notifier.emit());
    if (this.closed) return;
    if (this.options.watch || this.options.diagnostics)
      invokeLifecycleCallback(() =>
        this.options.onState?.(this.stateValue, snapshot.revision),
      );
    if (this.closed) return;
    if (this.options.diagnostics)
      invokeLifecycleCallback(() =>
        this.options.onDiagnostics?.(this.diagnosticsValue, snapshot.revision),
      );
  }

  private operationId(): Uint8Array {
    const value = this.makeOperationId();
    if (value.length !== 16 || value.every((byte) => byte === 0))
      throw new YasProtocolError("invalid LSP operation ID source");
    return new Uint8Array(value);
  }

  private assertOpen(): void {
    if (this.closed) throw new YasProtocolError("LSP workspace is closed");
  }

  private finish(reason: number): void {
    if (this.closed) return;
    this.closed = true;
    this.removeCatalog?.();
    this.removeCatalog = null;
    this.removeClosed?.();
    this.removeClosed = null;
    for (const queue of this.bufferQueues.values()) queue.pending = null;
    this.bufferQueues.clear();
    this.bufferFailures.clear();
    this.buffers.clear();
    this.retainedBufferBytes = 0;
    this.owner.forget(this);
    invokeLifecycleCallback(() => this.notifier.emit());
    invokeLifecycleCallback(() => this.options.onClosed?.(reason));
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
    else console.error("YAS LSP lifecycle callback failed", error);
  } catch {
    // Cleanup must not depend on host error reporting.
  }
}

function queryRecords(
  record: YasLspQueryRecord,
  defaultPath: string,
): YasNativeLspResultRecord[] {
  if (record.kind === "location") return [locationRecord(record)];
  if (record.kind === "hover")
    return [
      locationRecord(record.target),
      {
        kind: "markup",
        format: record.markupKind,
        text: decoder.decode(record.content),
      },
    ];
  if (record.kind === "symbol")
    return [
      {
        kind: "symbol",
        symKind: record.symbolKind + 1,
        flags: record.flags,
        depth: record.depth,
        line: record.range.start.line,
        col: record.range.start.byteColumn,
        endLine: record.range.end.line,
        endCol: record.range.end.byteColumn,
        name: record.name,
        path: record.path ? pathString(record.path) : defaultPath,
      },
    ];
  if (record.kind === "completion") {
    const range = record.replacementRange;
    return [
      {
        kind: "completion",
        itemKind: record.itemKind + 1,
        flags:
          (record.flags & g.YAS_LSP_COMPLETION_DEPRECATED
            ? LSP_COMPLETION_DEPRECATED
            : 0) |
          (record.flags & g.YAS_LSP_COMPLETION_SNIPPET_TEXT
            ? LSP_COMPLETION_SNIPPET
            : 0) |
          (record.flags & g.YAS_LSP_COMPLETION_PRESELECT
            ? LSP_COMPLETION_PRESELECT
            : 0),
        line: range?.start.line ?? 0,
        col: range?.start.byteColumn ?? 0,
        endLine: range?.end.line ?? 0,
        endCol: range?.end.byteColumn ?? 0,
        label: record.label,
        insert: decoder.decode(record.insertText),
        detail: record.detail,
      },
    ];
  }
  if (record.kind === "edit")
    return [
      {
        kind: "edit",
        flags: 0,
        hash: exactHash(record.expectedContentHash),
        line: record.range.start.line,
        col: record.range.start.byteColumn,
        endLine: record.range.end.line,
        endCol: record.range.end.byteColumn,
        newText: decoder.decode(record.replacement),
        path: pathString(record.path),
      },
    ];
  if (record.kind === "signature")
    return [
      {
        kind: "signature",
        flags: record.flags,
        activeParam: record.activeParameter,
        paramStart: record.parameterStart,
        paramEnd: record.parameterEnd,
        label: record.label,
        doc: record.documentation,
      },
    ];
  return [
    {
      kind: "action",
      actionKind: record.actionKind,
      title: record.title,
      flags: record.flags,
      disabledReason: record.disabledReason,
      edits: record.edits.flatMap((edit) =>
        queryRecords(edit, defaultPath),
      ) as Extract<YasNativeLspResultRecord, { kind: "edit" }>[],
    },
  ];
}

function locationRecord(
  record: Extract<YasLspQueryRecord, { kind: "location" }>,
): YasNativeLspResultRecord {
  return {
    kind: "location",
    flags: record.flags,
    hash: exactHash(record.contentHash),
    line: record.range.start.line,
    col: record.range.start.byteColumn,
    endLine: record.range.end.line,
    endCol: record.range.end.byteColumn,
    path: pathString(record.path),
  };
}

function workspaceStatus(status: number): number {
  if (status === g.YAS_STATUS_OK) return LSP_STATUS_OK;
  if (status === g.YAS_STATUS_NOT_FOUND) return LSP_STATUS_NOT_FOUND;
  if (status === g.YAS_STATUS_UNSUPPORTED) return LSP_STATUS_WRONG_TYPE;
  if (status === g.YAS_STATUS_INVALID) return LSP_STATUS_INVALID;
  if (status === g.YAS_STATUS_CANCELLED) return LSP_STATUS_CANCELLED;
  if (
    status === g.YAS_STATUS_BUSY ||
    status === g.YAS_STATUS_UNAVAILABLE ||
    status === g.YAS_STATUS_RATE_LIMITED
  )
    return LSP_STATUS_WARMING;
  if (status === g.YAS_STATUS_RESOURCE_EXHAUSTED) return LSP_STATUS_BUDGET;
  return LSP_STATUS_OTHER;
}

function wirePath(path: string): YasFsPath {
  if (path === "" || path === ".") return { components: [] };
  const normalized = path.replace(/\\/g, "/");
  if (normalized.startsWith("/"))
    throw new YasProtocolError("LSP document paths must be root-relative");
  return {
    components: normalized
      .split("/")
      .filter((component) => component.length !== 0 && component !== ".")
      .map((component) => {
        if (component === "..")
          throw new YasProtocolError("LSP path traverses above its workspace");
        return encoder.encode(component);
      }),
  };
}

function pathString(path: YasFsPath): string {
  return path.components
    .map((component) => decoder.decode(component))
    .join("/");
}

function exactHash(value: Uint8Array): Uint8Array {
  if (value.length !== 32)
    throw new YasProtocolError("LSP content hash is not 32 bytes");
  return new Uint8Array(value);
}

function cloneIdentity(value: YasLspBufferIdentity): YasLspBufferIdentity {
  return {
    ...value,
    contentHash: new Uint8Array(value.contentHash),
    extensions: value.extensions.map((extension) => ({
      ...extension,
      value: new Uint8Array(extension.value),
    })),
  };
}

function validateOpenOptions(options: YasNativeLspOpenOptions): void {
  const settle = options.diagLatencyMs ?? 0;
  if (
    !Number.isInteger(settle) ||
    settle < 0 ||
    settle > g.YAS_LSP_MAX_DIAGNOSTICS_SETTLE_MS
  )
    throw new YasProtocolError("LSP diagnostics settle delay is invalid");
}

function randomOperationId(): Uint8Array {
  const value = globalThis.crypto.getRandomValues(new Uint8Array(16));
  if (value.every((byte) => byte === 0)) value[0] = 1;
  return value;
}

async function defaultBlake3(bytes: Uint8Array): Promise<Uint8Array> {
  const { blake3_hash } = await import("@yas-run/browser");
  return blake3_hash(bytes);
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
