/** Workspace-facing Git operations backed directly by typed YAS Git. */

import * as model from "../gitModel";
import { Notifier } from "../reactive";
import type { SessionId } from "../types";
import * as g from "./generated";
import { decodeFsPath, type YasFsPath } from "./fs";
import {
  YasGitClient,
  type YasGitContentRecord,
  type YasGitEntityRecord,
  type YasGitObjectId,
  type YasGitQueryCursor,
  type YasGitQueryEndpoint,
  type YasGitQueryRecord,
  type YasGitRepository,
  type YasGitRepositorySource,
  type YasGitSnapshot,
  type YasGitWatchedQuery,
} from "./git";
import type { YasConnection } from "./session";
import { YasProtocolError, YasResultError } from "./wire";

const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { ignoreBOM: true });
/**
 * `YAS_GIT_MAX_QUERY_BYTES` is the family's ceiling on a single query page, not
 * the size of one. Proposing it per query charged 4 MiB of the session's
 * aggregate receive budget to a `git status` that answers in kilobytes, and a
 * workspace with a git panel open alongside a few terminals ran the budget dry
 * -- at which point Terminal and Surface views, which reserve all-or-nothing,
 * stop opening. Transfer replenishes consumed credit as records are read, so a
 * window sized to what the caller actually asked for costs a round trip at
 * worst, never a truncated page.
 */
const MAX_QUERY_CREDIT = BigInt(g.YAS_GIT_MAX_QUERY_BYTES);
const MIN_QUERY_CREDIT = 128n * 1024n;
const QUERY_CREDIT_PER_RECORD = 4096n;

function clampQueryCredit(bytes: bigint): bigint {
  if (bytes < MIN_QUERY_CREDIT) return MIN_QUERY_CREDIT;
  return bytes > MAX_QUERY_CREDIT ? MAX_QUERY_CREDIT : bytes;
}

/** Window for a query page, sized by its record limit when it has one. */
function queryCredit(maxRecords = 0): bigint {
  if (maxRecords <= 0) return MIN_QUERY_CREDIT;
  return clampQueryCredit(BigInt(maxRecords) * QUERY_CREDIT_PER_RECORD);
}

/** Window for content delivery, whose exact size the record already states. */
function contentCredit(record: YasGitContentRecord): bigint {
  return clampQueryCredit(record.nextOffset - record.offset);
}

export interface YasNativeGitCommitRecord {
  kind: "commit";
  flags: number;
  oid: model.GitOid;
  tree: model.GitOid;
  parents: model.GitOid[];
  authorTime: bigint;
  authorTz: number;
  committerTime: bigint;
  committerTz: number;
  authorName: string;
  authorEmail: string;
  committerName: string;
  committerEmail: string;
  message: string;
}

export interface YasNativeGitLogPathRecord {
  kind: "pathAt";
  otype: number;
  mode: number;
  oid: model.GitOid;
  path: string;
}

export type YasNativeGitLogRecord =
  | YasNativeGitCommitRecord
  | YasNativeGitLogPathRecord;

export interface YasNativeGitCommitsPage {
  status: number;
  flags: number;
  frontier: model.GitOid[];
  records: YasNativeGitLogRecord[];
}

export interface YasNativeGitLogPage extends YasNativeGitCommitsPage {
  /** Browser observation revision; the server-owned resource remains the repo. */
  updateRevision: bigint;
}

export interface YasNativeGitLogSubscription {
  close(): void;
}

export interface YasNativeGitOpenOptions extends Omit<
  model.GitOpenOptions,
  "parentRepoId" | "onState"
> {
  /** Exact server-issued handle for a parent repository when opening a submodule. */
  parentRepositoryHandle?: bigint;
  onState?: (state: model.GitStateMirror, revision: bigint) => void;
}

export type YasNativeGitDiscoverOptions = model.GitDiscoverOptions;
export type YasNativeGitFoundRepo = model.GitFoundRepo;

export interface YasNativeGitRepoHandle {
  readonly repositoryHandle: bigint;
  readonly repositoryRevision: bigint;
  readonly oidFormat: number;
  readonly repoFlags: number;
  readonly workdir: string;
  readonly gitdir: string;
  readonly state: model.GitStateMirror;
  readonly revision: number;
  subscribe(listener: () => void): () => void;
  log(
    request?: Partial<Omit<model.GitLogRequest, "nonce" | "repoId">>,
    options?: model.GitRequestOptions,
  ): Promise<YasNativeGitCommitsPage>;
  patch(
    old: model.GitEndpoint,
    next: model.GitEndpoint,
    options?: model.GitRequestOptions & {
      flags?: number;
      context?: number;
      path?: string;
      maxLen?: number;
      rename?: number;
      after?: string;
      afterPos?: number;
    },
  ): Promise<{
    flags: number;
    records: model.GitPatchRecord[];
    text: Uint8Array;
  }>;
  resolve(
    spec: string,
    options?: model.GitRequestOptions,
  ): Promise<{ tips: model.GitOid[]; hides: model.GitOid[] }>;
  worktrees(
    options?: model.GitRequestOptions & { afterPos?: number },
  ): Promise<model.GitWorktreeRecord[]>;
  watchLog(
    spec: string,
    options: model.GitLogWatchOptions,
    onUpdate: (page: YasNativeGitLogPage) => void,
  ): YasNativeGitLogSubscription;
  close(): void;
}

export interface YasNativeWorkspaceGitOptions {
  terminalHandle(sessionId: SessionId): bigint | undefined;
  operationId?: () => Uint8Array;
  client?: Pick<YasGitClient, "open" | "discover">;
}

export class YasNativeWorkspaceGit {
  private client: Pick<YasGitClient, "open" | "discover"> | null;
  private readonly handles = new Map<bigint, NativeGitRepository>();
  private readonly makeOperationId: () => Uint8Array;
  private readonly removeInvalidation: () => void;
  private generation = 0;
  private disposed = false;

  constructor(
    readonly connection: YasConnection,
    private readonly options: YasNativeWorkspaceGitOptions,
  ) {
    this.client = options.client ?? null;
    this.makeOperationId = options.operationId ?? randomOperationId;
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family !== undefined && family !== g.YAS_FAMILY_GIT) return;
      this.generation++;
      for (const handle of [...this.handles.values()]) handle.invalidate();
    });
  }

  async openRepo(
    path: string,
    options: YasNativeGitOpenOptions = {},
  ): Promise<YasNativeGitRepoHandle> {
    this.assertOpen();
    validateOpenOptions(options);
    const generation = this.generation;
    const native = await asGitRequest("open", () =>
      this.clientForUse().open({
        source: this.source(path, options),
        extensions: [],
      }),
    );
    if (this.disposed || generation !== this.generation) {
      await native.close().catch(() => undefined);
      throw new YasProtocolError(
        "native Workspace Git changed while OPEN was pending",
      );
    }
    const handle = new NativeGitRepository(
      this,
      native,
      options,
      this.makeOperationId,
    );
    this.handles.set(native.handle, handle);
    try {
      await handle.start();
      return handle;
    } catch (error) {
      this.forget(handle);
      await native.close().catch(() => undefined);
      throw error;
    }
  }

  async discoverRepos(
    path: string,
    options: YasNativeGitDiscoverOptions = {},
  ): Promise<YasNativeGitFoundRepo[]> {
    this.assertOpen();
    const generation = this.generation;
    const depth = options.depth ?? 0;
    const maxPages = options.maxPages ?? 64;
    if (!Number.isInteger(depth) || depth < 0 || depth > 0xff)
      throw new YasProtocolError("Git discovery depth is invalid");
    if (!Number.isInteger(maxPages) || maxPages <= 0)
      throw new YasProtocolError("Git discovery page limit is invalid");
    const source: YasGitRepositorySource = {
      kind: "platform-path",
      path: encoder.encode(path),
    };
    let cursor: YasGitQueryCursor = { kind: "start" };
    const found: YasNativeGitFoundRepo[] = [];
    const seen = new Set<string>();
    for (let pageNumber = 0; pageNumber < maxPages; pageNumber++) {
      throwIfAborted(options.signal, "discover");
      const page = await withAbort(
        asGitRequest("discover", () =>
          this.clientForUse().discover(source, {
            maxDepth: depth,
            flags:
              (options.nested ? g.YAS_GIT_DISCOVER_NESTED : 0) |
              (options.bare ? g.YAS_GIT_DISCOVER_BARE : 0),
            maxRecords: 256,
            cursor,
            initialReceiveCredit: queryCredit(256),
          }),
        ),
        options.signal,
        "discover",
      );
      if (this.disposed || generation !== this.generation)
        throw new YasProtocolError(
          "native Workspace Git changed while discovery was pending",
        );
      const batch: YasNativeGitFoundRepo[] = [];
      for (const record of await page.records()) {
        if (record.kind !== "discovery") continue;
        const gitdir = text(record.gitDir);
        if (seen.has(gitdir)) continue;
        seen.add(gitdir);
        batch.push({
          workdir: text(record.worktreePath),
          gitdir,
          bare: Boolean(record.flags & g.YAS_GIT_DISCOVERY_BARE),
          linked: Boolean(record.flags & g.YAS_GIT_DISCOVERY_LINKED),
          submodule: Boolean(record.flags & g.YAS_GIT_DISCOVERY_SUBMODULE),
        });
      }
      found.push(...batch);
      options.onPage?.(batch);
      if (!(page.flags & g.YAS_GIT_QUERY_PAGE_MORE)) return found;
      if (page.nextCursor.kind !== "platform-path")
        throw new YasProtocolError("Git discovery returned the wrong cursor");
      cursor = page.nextCursor;
    }
    throw new model.GitStatusError(
      "discover",
      model.GIT_STATUS_BUDGET,
      "page limit exhausted",
    );
  }

  private clientForUse(): Pick<YasGitClient, "open" | "discover"> {
    return (this.client ??= new YasGitClient(this.connection));
  }

  forget(handle: NativeGitRepository): void {
    if (this.handles.get(handle.repositoryHandle) === handle)
      this.handles.delete(handle.repositoryHandle);
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.generation++;
    this.removeInvalidation();
    for (const handle of [...this.handles.values()]) handle.close();
    this.handles.clear();
    const client = this.client as { dispose?: () => void } | null;
    client?.dispose?.();
    this.client = null;
  }

  private source(
    path: string,
    options: YasNativeGitOpenOptions,
  ): YasGitRepositorySource {
    if (
      options.fromSessionId !== undefined &&
      options.parentRepositoryHandle !== undefined
    )
      throw new YasProtocolError(
        "Git open cannot use terminal cwd and submodule context together",
      );
    if (options.fromSessionId !== undefined) {
      const terminalHandle = this.options.terminalHandle(options.fromSessionId);
      if (terminalHandle === undefined)
        throw new YasProtocolError("Git source terminal is no longer present");
      return {
        kind: "terminal-cwd",
        terminalHandle,
        suffix: wirePath(path),
      };
    }
    if (options.parentRepositoryHandle !== undefined) {
      if (!this.handles.has(options.parentRepositoryHandle))
        throw new YasProtocolError("Git parent repository is no longer open");
      return {
        kind: "submodule",
        parentRepository: options.parentRepositoryHandle,
        path: wirePath(path),
      };
    }
    return { kind: "platform-path", path: encoder.encode(path) };
  }

  private assertOpen(): void {
    if (this.disposed)
      throw new YasProtocolError("native Workspace Git is closed");
  }
}

class NativeGitRepository implements YasNativeGitRepoHandle {
  readonly state = new model.GitStateMirror();
  readonly oidFormat: number;
  readonly repoFlags: number;
  readonly workdir: string;
  readonly gitdir: string;
  private readonly notifier = new Notifier();
  private readonly watchedLogs = new Set<NativeGitLogSubscription>();
  private removeCatalog: (() => void) | null = null;
  private removeClosed: (() => void) | null = null;
  private lastCatalogRevision = 0n;
  private closed = false;

  constructor(
    private readonly owner: YasNativeWorkspaceGit,
    readonly native: YasGitRepository,
    private readonly options: YasNativeGitOpenOptions,
    private readonly makeOperationId: () => Uint8Array,
  ) {
    this.oidFormat = native.opened.objectAlgorithm;
    this.repoFlags = native.opened.repositoryFlags;
    this.workdir = text(native.opened.canonicalWorktreePath);
    this.gitdir = text(native.opened.canonicalGitDir);
  }

  get repositoryHandle(): bigint {
    return this.native.handle;
  }

  get repositoryRevision(): bigint {
    return this.native.opened.repositoryRevision;
  }

  get revision(): number {
    return this.notifier.revision;
  }

  subscribe = this.notifier.subscribe;

  async start(): Promise<void> {
    this.removeClosed = this.native.onClosed((event) =>
      this.finish(
        event.detail === "Git session invalidated"
          ? model.GIT_CLOSED_CONNECTION_LOST
          : event.reason,
      ),
    );
    if (!wantsState(this.options)) return;
    const snapshot = await this.native.list({
      datasets: watchDatasets(this.options),
      refsSettleMs: this.options.refsLatencyMs,
      statusSettleMs: this.options.statusLatencyMs,
      refPrefixes: sortedRefPrefixes(this.options.refPrefixes),
    });
    if (this.closed) return;
    this.applySnapshot(snapshot);
    this.removeCatalog = this.native.catalog.subscribe((next) => {
      if (next.revision === 0n || next.revision === this.lastCatalogRevision)
        return;
      this.applySnapshot(next);
    });
  }

  log(
    request: Partial<Omit<model.GitLogRequest, "nonce" | "repoId">> = {},
    options: model.GitRequestOptions = {},
  ): Promise<YasNativeGitCommitsPage> {
    return this.request("log", options.signal, async () => {
      const page = await this.native.query(
        {
          kind: "log",
          spec: new Uint8Array(),
          tips: (request.tips ?? []).map((oid) => this.toNativeOid(oid)),
          hides: (request.hides ?? []).map((oid) => this.toNativeOid(oid)),
          path: request.path ? wirePath(request.path) : undefined,
          flags: request.flags ?? 0,
        },
        {
          maxRecords: request.limit ?? 0,
          initialReceiveCredit: queryCredit(request.limit ?? 0),
        },
      );
      return {
        status: model.GIT_STATUS_OK,
        flags:
          page.flags & g.YAS_GIT_QUERY_PAGE_MORE ? model.GIT_COMMITS_MORE : 0,
        frontier: logFrontier(page.nextCursor),
        records: (await page.records()).flatMap(commitRecord),
      };
    });
  }

  patch(
    old: model.GitEndpoint,
    next: model.GitEndpoint,
    options: model.GitRequestOptions & {
      flags?: number;
      context?: number;
      path?: string;
      maxLen?: number;
      rename?: number;
      after?: string;
      afterPos?: number;
    } = {},
  ): Promise<{
    flags: number;
    records: model.GitPatchRecord[];
    text: Uint8Array;
  }> {
    return this.request("patch", options.signal, async () => {
      const textMode = Boolean((options.flags ?? 0) & model.GIT_PATCH_TEXT);
      const cursor =
        options.after || options.afterPos
          ? ({
              kind: "patch",
              path: wirePath(options.after ?? ""),
              position: BigInt(options.afterPos ?? 0),
            } as const)
          : undefined;
      const page = await this.native.query(
        {
          kind: "patch",
          left: this.endpoint(old),
          right: this.endpoint(next),
          path: options.path ? wirePath(options.path) : undefined,
          contextLines: options.context ?? 0,
          renameThreshold: options.rename ?? 0,
          maxBytes: options.maxLen ?? 0,
          flags: options.flags ?? 0,
        },
        { cursor, initialReceiveCredit: queryCredit() },
      );
      const records: model.GitPatchRecord[] = [];
      let content: Uint8Array = new Uint8Array();
      for (const record of await page.records()) {
        if (record.kind === "patch")
          content = new Uint8Array(
            await this.native.content(record, contentCredit(record)),
          );
        else if (record.kind === "patch-file")
          records.push({
            kind: "file",
            st: diffStatus(record.status),
            similarity: record.similarityPercent,
            flags: record.flags,
            oldPath: record.oldPath ? pathText(record.oldPath) : "",
            newPath: record.newPath ? pathText(record.newPath) : "",
          });
        else if (record.kind === "patch-row")
          records.push({
            kind: "row",
            oldLine: record.oldLine,
            newLine: record.newLine,
            oldText: new Uint8Array(record.oldText),
            newText: new Uint8Array(record.newText),
            oldSpans: record.oldSpans.map(({ start, length }) => [
              start,
              length,
            ]),
            newSpans: record.newSpans.map(({ start, length }) => [
              start,
              length,
            ]),
          });
        else if (record.kind === "patch-gap")
          records.push({
            kind: "gap",
            oldLine: record.oldLine,
            newLine: record.newLine,
          });
        else if (record.kind === "patch-base")
          records.push({ kind: "base", oid: nativeOid(record.object) });
      }
      appendPatchCursor(records, page.flags, page.nextCursor);
      return {
        flags:
          (textMode ? 0 : model.GIT_PATCH_STRUCTURED) |
          (page.flags & g.YAS_GIT_QUERY_PAGE_MORE
            ? model.GIT_PATCH_TRUNCATED
            : 0),
        records,
        text: content,
      };
    });
  }

  resolve(
    spec: string,
    options: model.GitRequestOptions = {},
  ): Promise<{ tips: model.GitOid[]; hides: model.GitOid[] }> {
    return this.request("resolve", options.signal, async () => {
      const page = await this.native.query(
        { kind: "resolve", spec: encoder.encode(spec) },
        { initialReceiveCredit: queryCredit() },
      );
      const tips: model.GitOid[] = [];
      const hides: model.GitOid[] = [];
      for (const record of await page.records()) {
        if (record.kind !== "object") continue;
        if (record.role === g.YAS_GIT_OBJECT_ROLE_TIP)
          tips.push(nativeOid(record.object));
        else if (record.role === g.YAS_GIT_OBJECT_ROLE_HIDE)
          hides.push(nativeOid(record.object));
      }
      return { tips, hides };
    });
  }

  worktrees(
    options: model.GitRequestOptions & { afterPos?: number } = {},
  ): Promise<model.GitWorktreeRecord[]> {
    return this.request("worktrees", options.signal, async () => {
      const page = await this.native.query(
        { kind: "worktrees" },
        {
          cursor: options.afterPos
            ? { kind: "position", position: BigInt(options.afterPos) }
            : undefined,
          initialReceiveCredit: queryCredit(),
        },
      );
      const output: model.GitWorktreeRecord[] = [];
      for (const record of await page.records())
        if (record.kind === "worktree")
          output.push({
            kind: "tree",
            flags: worktreeFlags(record.flags),
            oid: nativeOid(record.head),
            path: text(record.path),
            branch: text(record.branch),
            lockReason: record.lockReason,
          });
      appendPositionCursor(output, page.flags, page.nextCursor);
      return output;
    });
  }

  watchLog(
    spec: string,
    options: model.GitLogWatchOptions,
    onUpdate: (page: YasNativeGitLogPage) => void,
  ): YasNativeGitLogSubscription {
    const subscription = new NativeGitLogSubscription(
      this,
      spec,
      options,
      onUpdate,
    );
    this.watchedLogs.add(subscription);
    subscription.start();
    return subscription;
  }

  close(): void {
    if (this.closed) return;
    for (const subscription of [...this.watchedLogs]) subscription.close();
    this.finish(model.GIT_CLOSED_CLIENT_REQUEST);
    void this.native.close().catch(() => undefined);
  }

  invalidate(): void {
    this.finish(model.GIT_CLOSED_CONNECTION_LOST);
  }

  forgetLog(subscription: NativeGitLogSubscription): void {
    this.watchedLogs.delete(subscription);
  }

  private applySnapshot(snapshot: YasGitSnapshot): void {
    if (this.closed || snapshot.revision === this.lastCatalogRevision) return;
    this.lastCatalogRevision = snapshot.revision;
    applyStateSnapshot(this.state, snapshot);
    invokeLifecycleCallback(() =>
      this.options.onState?.(this.state, snapshot.revision),
    );
    if (this.closed) return;
    invokeLifecycleCallback(() => this.notifier.emit());
  }

  private finish(reason: number): void {
    if (this.closed) return;
    this.closed = true;
    this.removeCatalog?.();
    this.removeCatalog = null;
    this.removeClosed?.();
    this.removeClosed = null;
    for (const subscription of [...this.watchedLogs]) subscription.closeLocal();
    this.watchedLogs.clear();
    this.owner.forget(this);
    invokeLifecycleCallback(() => this.options.onClosed?.(reason));
    invokeLifecycleCallback(() => this.notifier.emit());
  }

  private request<T>(
    op: string,
    signal: AbortSignal | undefined,
    work: () => Promise<T>,
  ): Promise<T> {
    if (this.closed)
      return Promise.reject(
        new model.GitStatusError(op, model.GIT_STATUS_UNKNOWN_ID),
      );
    return withAbort(asGitRequest(op, work), signal, op);
  }

  private endpoint(value: model.GitEndpoint): YasGitQueryEndpoint {
    if (value.kind === model.GIT_ENDPOINT_EMPTY) return { kind: "empty" };
    if (value.kind === model.GIT_ENDPOINT_COMMIT)
      return { kind: "commit", object: this.toNativeOid(value.oid) };
    if (value.kind === model.GIT_ENDPOINT_TREE)
      return { kind: "tree", object: this.toNativeOid(value.oid) };
    if (value.kind === model.GIT_ENDPOINT_INDEX) return { kind: "index" };
    if (value.kind === model.GIT_ENDPOINT_WORKTREE) return { kind: "worktree" };
    if (value.kind === model.GIT_ENDPOINT_MERGE_BASE)
      return { kind: "merge-base", object: this.toNativeOid(value.oid) };
    throw new YasProtocolError("unknown Git endpoint kind");
  }

  private toNativeOid(value: model.GitOid): YasGitObjectId {
    if (value.length !== 32 || model.gitOidIsZero(value))
      throw new YasProtocolError("invalid Git object ID");
    const length = this.oidFormat === model.GIT_OID_FORMAT_SHA1 ? 20 : 32;
    if (length === 20 && value.subarray(20).some((byte) => byte !== 0))
      throw new YasProtocolError("SHA-1 object ID has nonzero padding");
    return {
      algorithm: this.oidFormat,
      bytes: new Uint8Array(value.subarray(0, length)),
    };
  }
}

class NativeGitLogSubscription implements YasNativeGitLogSubscription {
  private native: YasGitWatchedQuery | null = null;
  private closed = false;
  private updateRevision = 0n;
  private delivery = Promise.resolve();
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private retryDelayMs = 100;

  constructor(
    private readonly repository: NativeGitRepository,
    private readonly spec: string,
    private readonly options: model.GitLogWatchOptions,
    private readonly onUpdate: (page: YasNativeGitLogPage) => void,
  ) {}

  start(): void {
    void this.repository.native
      .watchQuery(
        {
          kind: "log",
          spec: encoder.encode(this.spec),
          tips: [],
          hides: [],
          flags: this.options.flags ?? 0,
        },
        (update) => {
          this.delivery = this.delivery.then(() => this.deliver(update));
        },
        {
          initialCredit: queryCredit(this.options.limit ?? 0),
          maxRecords: this.options.limit ?? 0,
        },
      )
      .then((native) => {
        if (this.closed) void native.close().catch(() => undefined);
        else {
          this.native = native;
          this.retryDelayMs = 100;
        }
      })
      .catch((error) => {
        if (this.closed) return;
        if (isTransientGitWatchStartError(error)) {
          const delay = this.retryDelayMs;
          this.retryDelayMs = Math.min(delay * 2, 2_000);
          this.retryTimer = setTimeout(() => {
            this.retryTimer = null;
            if (!this.closed) this.start();
          }, delay);
          return;
        }
        invokeLifecycleCallback(() =>
          this.onUpdate({
            updateRevision: ++this.updateRevision,
            status:
              error instanceof YasResultError
                ? coreStatusToGit(error.status)
                : model.GIT_STATUS_OTHER,
            flags: 0,
            frontier: [],
            records: [],
          }),
        );
        this.closeLocal();
      });
  }

  close(): void {
    if (this.closed) return;
    this.closed = true;
    if (this.retryTimer !== null) clearTimeout(this.retryTimer);
    this.retryTimer = null;
    this.repository.forgetLog(this);
    void this.native?.close().catch(() => undefined);
    this.native = null;
  }

  closeLocal(): void {
    if (this.closed) return;
    this.closed = true;
    if (this.retryTimer !== null) clearTimeout(this.retryTimer);
    this.retryTimer = null;
    this.repository.forgetLog(this);
    this.native = null;
  }

  private async deliver(
    update: import("./git").YasGitWatchedQueryUpdate,
  ): Promise<void> {
    if (this.closed) return;
    if (!update.page) {
      invokeLifecycleCallback(() =>
        this.onUpdate({
          updateRevision: ++this.updateRevision,
          status: coreStatusToGit(update.status),
          flags: 0,
          frontier: [],
          records: [],
        }),
      );
      return;
    }
    const page = update.page;
    const records = (await page.records()).flatMap(commitRecord);
    if (this.closed) return;
    invokeLifecycleCallback(() =>
      this.onUpdate({
        updateRevision: ++this.updateRevision,
        status: model.GIT_STATUS_OK,
        flags:
          page.flags & g.YAS_GIT_QUERY_PAGE_MORE ? model.GIT_COMMITS_MORE : 0,
        frontier: logFrontier(page.nextCursor),
        records,
      }),
    );
  }
}

export function isTransientGitWatchStartError(error: unknown): boolean {
  return (
    error instanceof YasResultError &&
    (error.status === g.YAS_STATUS_RESOURCE_EXHAUSTED ||
      error.status === g.YAS_STATUS_UNAVAILABLE ||
      error.status === g.YAS_STATUS_CANCELLED)
  );
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
    else console.error("YAS Git lifecycle callback failed", error);
  } catch {
    // Cleanup must not depend on host error reporting.
  }
}

function commitRecord(record: YasGitQueryRecord): YasNativeGitLogRecord[] {
  if (record.kind === "commit")
    return [
      {
        kind: "commit",
        flags: record.flags,
        oid: nativeOid(record.object),
        tree: nativeOid(record.tree),
        parents: record.parents.map(nativeOid),
        authorTime: record.authoredUnixSeconds,
        authorTz: record.authorTimezoneMinutes,
        committerTime: record.committedUnixSeconds,
        committerTz: record.committerTimezoneMinutes,
        authorName: text(record.authorName),
        authorEmail: text(record.authorEmail),
        committerName: text(record.committerName),
        committerEmail: text(record.committerEmail),
        message: text(record.message),
      },
    ];
  if (record.kind === "log-path")
    return [
      {
        kind: "pathAt",
        otype: treeObjectType(record.entryKind),
        mode: record.mode,
        oid: nativeOid(record.object),
        path: pathText(record.path),
      },
    ];
  return [];
}

function applyStateSnapshot(
  mirror: model.GitStateMirror,
  snapshot: YasGitSnapshot,
): void {
  mirror.head = null;
  mirror.refs = new Map();
  mirror.op = null;
  mirror.status = [];
  mirror.upstreams = new Map();
  mirror.stashes = [];
  mirror.remotes = new Map();
  mirror.worktreeGen = { count: 0, digest: 0n };
  mirror.flags = 0;
  for (const entity of snapshot.entities) applyEntity(mirror, entity);
  mirror.status.sort((left, right) => left.path.localeCompare(right.path));
  mirror.stashes.sort((left, right) => left.index - right.index);
}

function applyEntity(
  mirror: model.GitStateMirror,
  entity: YasGitEntityRecord,
): void {
  const body = entity.body;
  if (body.kind === "head") {
    mirror.head = {
      flags: body.flags,
      oid: nativeOid(body.object),
      name: text(body.symbolicTarget),
    };
  } else if (body.kind === "ref") {
    mirror.refs.set(text(entity.key), {
      flags: body.flags,
      oid: nativeOid(body.object),
      peeled: nativeOid(body.peeled),
      target: text(body.symbolicTarget),
    });
  } else if (body.kind === "remote") {
    mirror.remotes.set(text(entity.key), {
      flags: body.flags,
      fetchUrl: text(body.fetchUrl),
      pushUrl: text(body.pushUrl),
    });
  } else if (body.kind === "operation") {
    mirror.op = {
      op: body.operationKind,
      oid: nativeOid(body.head),
      detail: body.detail,
    };
  } else if (body.kind === "status") {
    mirror.status.push({
      staged: statusLetter(body.indexStatus),
      unstaged: statusLetter(body.worktreeStatus),
      flags:
        body.flags & g.YAS_GIT_STATE_STATUS_CONFLICTED
          ? model.GIT_STATUS_ENTRY_CONFLICTED
          : 0,
      oid: nativeOid(body.content),
      oldPath: body.oldPath ? pathText(body.oldPath) : "",
      path: pathText(decodeFsPath(entity.key)),
    });
  } else if (body.kind === "upstream") {
    mirror.upstreams.set(text(entity.key), {
      flags: body.flags,
      ahead: body.ahead,
      behind: body.behind,
      upstream: text(body.upstream),
    });
  } else if (body.kind === "stash") {
    mirror.stashes.push({
      index: littleU32(entity.key),
      oid: nativeOid(body.object),
      time: body.createdUnixSeconds,
      tz: body.timezoneMinutes,
      message: text(body.message),
    });
  } else {
    mirror.worktreeGen = { count: body.count, digest: body.digest };
  }
}

function wantsState(options: YasNativeGitOpenOptions): boolean {
  return Boolean(
    options.watch ||
    options.status ||
    options.untracked ||
    options.ignored ||
    options.tracking ||
    options.remotes,
  );
}

function watchDatasets(options: YasNativeGitOpenOptions): number {
  let datasets =
    g.YAS_GIT_WATCH_HEAD |
    g.YAS_GIT_WATCH_REFS |
    g.YAS_GIT_WATCH_OPERATION |
    g.YAS_GIT_WATCH_STASHES |
    g.YAS_GIT_WATCH_WORKTREE_GENERATION;
  if (options.status || options.untracked || options.ignored)
    datasets |= g.YAS_GIT_WATCH_STATUS;
  if (options.tracking) datasets |= g.YAS_GIT_WATCH_UPSTREAMS;
  if (options.remotes) datasets |= g.YAS_GIT_WATCH_REMOTES;
  return datasets;
}

function sortedRefPrefixes(
  values: readonly string[] | undefined,
): Uint8Array[] {
  return (values ?? [])
    .map((value) => encoder.encode(value))
    .sort(compareBytes);
}

function validateOpenOptions(options: YasNativeGitOpenOptions): void {
  if (options.ignored && !options.untracked)
    throw new YasProtocolError("Git ignored status requires untracked status");
  for (const [name, value] of [
    ["refs", options.refsLatencyMs],
    ["status", options.statusLatencyMs],
  ] as const)
    if (
      value !== undefined &&
      (!Number.isInteger(value) || value < 0 || value > 0xffff)
    )
      throw new YasProtocolError(`Git ${name} settle delay is invalid`);
}

function wirePath(path: string): YasFsPath {
  if (path === "" || path === ".") return { components: [] };
  const normalized = path.replace(/\\/g, "/");
  if (normalized.startsWith("/"))
    throw new YasProtocolError("Git query paths must be root-relative");
  return {
    components: normalized
      .split("/")
      .filter((component) => component !== "" && component !== ".")
      .map((component) => {
        if (component === "..")
          throw new YasProtocolError("Git path traverses above its root");
        return encoder.encode(component);
      }),
  };
}

function pathText(path: YasFsPath): string {
  return path.components.map(text).join("/");
}

function text(value: Uint8Array): string {
  return decoder.decode(value);
}

function nativeOid(value?: YasGitObjectId): model.GitOid {
  const result = new Uint8Array(32);
  if (value) result.set(value.bytes);
  return result;
}

function treeObjectType(kind: number): number {
  if (kind < g.YAS_GIT_TREE_BLOB || kind > g.YAS_GIT_TREE_COMMIT)
    throw new YasProtocolError("unknown Git tree object kind");
  return 3 - kind;
}

function diffStatus(status: number): number {
  return [65, 77, 68, 82, 67][status] ?? 0;
}

function statusLetter(status: number): number {
  return [0, 65, 77, 68, 82, 67, 84, 85, 63, 33][status] ?? 0;
}

function worktreeFlags(flags: number): number {
  return (
    (flags & g.YAS_GIT_WORKTREE_MAIN ? model.GIT_WORKTREE_MAIN : 0) |
    (flags & g.YAS_GIT_WORKTREE_CURRENT ? model.GIT_WORKTREE_CURRENT : 0) |
    (flags & g.YAS_GIT_WORKTREE_LOCKED ? model.GIT_WORKTREE_LOCKED : 0) |
    (flags & g.YAS_GIT_WORKTREE_PRUNABLE ? model.GIT_WORKTREE_PRUNABLE : 0) |
    (flags & g.YAS_GIT_WORKTREE_DETACHED ? model.GIT_WORKTREE_DETACHED : 0) |
    (flags & g.YAS_GIT_WORKTREE_BARE ? model.GIT_WORKTREE_BARE : 0)
  );
}

function logFrontier(cursor: YasGitQueryCursor): model.GitOid[] {
  return cursor.kind === "log-frontier" ? cursor.objects.map(nativeOid) : [];
}

function appendPatchCursor(
  output: model.GitPatchRecord[],
  flags: number,
  cursor: YasGitQueryCursor,
): void {
  if (!(flags & g.YAS_GIT_QUERY_PAGE_MORE)) return;
  if (cursor.kind !== "patch")
    throw new YasProtocolError("Git PATCH returned the wrong cursor");
  output.push({
    kind: "cursor",
    after: pathText(cursor.path),
    pos: cursor.position,
  });
}

function appendPositionCursor(
  output: model.GitWorktreeRecord[],
  flags: number,
  cursor: YasGitQueryCursor,
): void {
  if (!(flags & g.YAS_GIT_QUERY_PAGE_MORE)) return;
  if (cursor.kind !== "position")
    throw new YasProtocolError("Git query returned the wrong position cursor");
  output.push({ kind: "cursor", after: "", pos: cursor.position });
}

async function asGitRequest<T>(op: string, work: () => Promise<T>): Promise<T> {
  try {
    return await work();
  } catch (error) {
    if (error instanceof model.GitStatusError) throw error;
    if (error instanceof YasResultError)
      throw new model.GitStatusError(
        op,
        coreStatusToGit(error.status),
        text(error.detail),
      );
    throw error;
  }
}

function coreStatusToGit(status: number): number {
  if (status === g.YAS_STATUS_OK) return model.GIT_STATUS_OK;
  if (status === g.YAS_STATUS_NOT_FOUND) return model.GIT_STATUS_NOT_FOUND;
  if (status === g.YAS_STATUS_UNSUPPORTED) return model.GIT_STATUS_WRONG_TYPE;
  if (status === g.YAS_STATUS_INVALID) return model.GIT_STATUS_INVALID;
  if (status === g.YAS_STATUS_CANCELLED) return model.GIT_STATUS_CANCELLED;
  if (status === g.YAS_STATUS_RESOURCE_EXHAUSTED)
    return model.GIT_STATUS_BUDGET;
  if (status === g.YAS_STATUS_CONFLICT) return model.GIT_STATUS_CONFLICT;
  return model.GIT_STATUS_OTHER;
}

function withAbort<T>(
  promise: Promise<T>,
  signal: AbortSignal | undefined,
  op: string,
): Promise<T> {
  if (!signal) return promise;
  if (signal.aborted)
    return Promise.reject(
      new model.GitStatusError(op, model.GIT_STATUS_CANCELLED),
    );
  return new Promise<T>((resolve, reject) => {
    const abort = () =>
      reject(new model.GitStatusError(op, model.GIT_STATUS_CANCELLED));
    signal.addEventListener("abort", abort, { once: true });
    promise.then(
      (value) => {
        signal.removeEventListener("abort", abort);
        resolve(value);
      },
      (error) => {
        signal.removeEventListener("abort", abort);
        reject(error);
      },
    );
  });
}

function throwIfAborted(signal: AbortSignal | undefined, op: string): void {
  if (signal?.aborted)
    throw new model.GitStatusError(op, model.GIT_STATUS_CANCELLED);
}

function randomOperationId(): Uint8Array {
  const value = globalThis.crypto.getRandomValues(new Uint8Array(16));
  if (value.every((byte) => byte === 0)) value[0] = 1;
  return value;
}

function littleU32(value: Uint8Array): number {
  if (value.length !== 4)
    throw new YasProtocolError("Git stash key is not a u32");
  return new DataView(
    value.buffer,
    value.byteOffset,
    value.byteLength,
  ).getUint32(0, true);
}

function compareBytes(left: Uint8Array, right: Uint8Array): number {
  const length = Math.min(left.length, right.length);
  for (let index = 0; index < length; index++) {
    const difference = left[index]! - right[index]!;
    if (difference !== 0) return difference;
  }
  return left.length - right.length;
}
