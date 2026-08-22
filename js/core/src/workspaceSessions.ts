/** Durable, user-owned workspaces stored in the home server KV. */

import { LAYOUT_DSL_MAX_PANES, parseDSL } from "./layout/dsl";
import type {
  WorkspaceSessionHash,
  WorkspaceSessionKv,
  WorkspaceSessionKvEntry,
  WorkspaceSessionKvWatch,
  WorkspaceSessionOwnedKv,
} from "./workspaceSessionKv";
import {
  WorkspaceSessionKvConflictError,
  copyWorkspaceSessionHash,
  workspaceSessionHashesEqual,
} from "./workspaceSessionKv";
import { YasNativeWorkspaceKv } from "./yas/nativeWorkspaceKv";
import type { YasConnection } from "./yas/session";

export type {
  WorkspaceSessionHash,
  WorkspaceSessionKv,
  WorkspaceSessionKvDeleteOptions,
  WorkspaceSessionKvEntry,
  WorkspaceSessionKvMirror,
  WorkspaceSessionKvPutOptions,
  WorkspaceSessionKvWatch,
  WorkspaceSessionKvWatchOptions,
  WorkspaceSessionOwnedKv,
} from "./workspaceSessionKv";
export { WorkspaceSessionKvConflictError } from "./workspaceSessionKv";

const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });

export const WORKSPACE_SESSION_VERSION = 1 as const;
export const WORKSPACE_SESSION_KEY_PREFIX = "ui/workspace-sessions/v1/";
export const WORKSPACE_SESSION_MAX_DOCUMENT_BYTES = 4 * 1024 * 1024;
export const WORKSPACE_SESSION_MAX_CATALOG_ENTRIES = 4_096;
export const WORKSPACE_SESSION_MAX_RETAINED_BYTES = 256 * 1024 * 1024;
export const WORKSPACE_SESSION_MAX_NAME_BYTES = 256;
/** Matches the browser Relay catalogue's admitted route bound. */
export const WORKSPACE_SESSION_MAX_REMOTES = 128;
/** 255 admitted UI characters, including multi-byte Unicode route names. */
export const WORKSPACE_SESSION_MAX_REMOTE_BYTES = 1_024;
export const WORKSPACE_SESSION_MAX_LAYOUT_NAME_BYTES = 256;
export const WORKSPACE_SESSION_MAX_LAYOUT_DSL_BYTES = 256 * 1024;
export const WORKSPACE_SESSION_MAX_ASSIGNMENTS = LAYOUT_DSL_MAX_PANES;
export const WORKSPACE_SESSION_MAX_PANE_ID_BYTES = 256;
export const WORKSPACE_SESSION_MAX_REFERENCE_BYTES = 16 * 1024;
export const WORKSPACE_SESSION_MAX_EXPANDED_SECTIONS = 64;
export const WORKSPACE_SESSION_MAX_PANEL_ID_BYTES = 128;
export const WORKSPACE_SESSION_MAX_PATH_BYTES = 16 * 1024;

const WATCH_INLINE_MAX = 32 * 1024;
const DEFAULT_CAS_RETRIES = 3;
const UUID_RE =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

export interface WorkspaceSessionLayout {
  name: string;
  dsl: string;
}

export type WorkspaceSessionProjectSelection =
  | { kind: "focused" }
  | { kind: "declared"; name: string }
  | {
      kind: "worktree";
      connectionId: string;
      path: string;
      label: string;
    };

export interface WorkspaceSessionPanels {
  leftOpen: boolean;
  previewOpen: boolean;
  expandedSections: string[];
  project: WorkspaceSessionProjectSelection | null;
  musterExpanded: boolean;
  debugOpen: boolean;
}

export interface WorkspaceSessionWorkspace {
  layout: WorkspaceSessionLayout | null;
  /** Pane id to a stable encoded terminal/surface/tile reference. */
  assignments: Record<string, string>;
  focusedPaneId: string | null;
  /** Stable encoded main/focus reference; never an ephemeral browser id. */
  main: string | null;
  panels: WorkspaceSessionPanels;
}

export interface StoredWorkspaceSession {
  version: 1;
  id: string;
  name: string;
  createdAtUnixMs: number;
  updatedAtUnixMs: number;
  /** Ordered, unique Relay route names. Home is implicit. */
  activeRemotes: string[];
  workspace: WorkspaceSessionWorkspace;
}

export interface WorkspaceSessionPanelsPatch {
  leftOpen?: boolean;
  previewOpen?: boolean;
  expandedSections?: readonly string[];
  project?: WorkspaceSessionProjectSelection | null;
  musterExpanded?: boolean;
  debugOpen?: boolean;
}

export interface WorkspaceSessionWorkspacePatch {
  layout?: WorkspaceSessionLayout | null;
  assignments?: Readonly<Record<string, string>>;
  focusedPaneId?: string | null;
  main?: string | null;
  panels?: WorkspaceSessionPanelsPatch;
}

/** A semantic patch. It is reapplied to the latest record after a CAS race. */
export interface WorkspaceSessionPatch {
  name?: string;
  activeRemotes?: readonly string[];
  workspace?: WorkspaceSessionWorkspacePatch;
}

export interface CreateStoredWorkspaceSessionOptions {
  id: string;
  name?: string;
  nowUnixMs?: number;
  activeRemotes?: readonly string[];
  workspace?: WorkspaceSessionWorkspace;
}

export interface CreateWorkspaceSessionInput {
  /** Omit to generate a cryptographically random UUID. */
  id?: string;
  name?: string;
  activeRemotes?: readonly string[];
  workspace?: WorkspaceSessionWorkspace;
}

export class WorkspaceSessionValidationError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "WorkspaceSessionValidationError";
  }
}

export class WorkspaceSessionNotFoundError extends Error {
  constructor(readonly id: string) {
    super(`Workspace ${id} was not found`);
    this.name = "WorkspaceSessionNotFoundError";
  }
}

export function isWorkspaceSessionId(value: unknown): value is string {
  return typeof value === "string" && UUID_RE.test(value);
}

export function workspaceSessionKey(id: string): string {
  assertSessionId(id);
  return `${WORKSPACE_SESSION_KEY_PREFIX}${id}`;
}

function workspaceSessionIdFromKey(key: string): string | null {
  if (!key.startsWith(WORKSPACE_SESSION_KEY_PREFIX)) return null;
  const id = key.slice(WORKSPACE_SESSION_KEY_PREFIX.length);
  return isWorkspaceSessionId(id) && workspaceSessionKey(id) === key
    ? id
    : null;
}

/** First positive numeric workspace name not already present. */
export function nextWorkspaceName(
  workspaces: readonly Pick<StoredWorkspaceSession, "name">[],
): string {
  const used = new Set(
    workspaces.flatMap(({ name }) => (/^[1-9]\d*$/.test(name) ? [name] : [])),
  );
  let candidate = 1;
  while (used.has(String(candidate))) candidate++;
  return String(candidate);
}

/** Conservative defaults for callers creating an empty workspace. */
export function createDefaultStoredWorkspaceSession(
  options: CreateStoredWorkspaceSessionOptions,
): StoredWorkspaceSession {
  const now = options.nowUnixMs ?? Date.now();
  return parseStoredWorkspaceSession({
    version: WORKSPACE_SESSION_VERSION,
    id: options.id,
    name: options.name ?? "1",
    createdAtUnixMs: now,
    updatedAtUnixMs: now,
    activeRemotes: options.activeRemotes ?? [],
    workspace: options.workspace ?? {
      layout: null,
      assignments: {},
      focusedPaneId: null,
      main: null,
      panels: {
        leftOpen: false,
        previewOpen: false,
        expandedSections: [],
        project: null,
        musterExpanded: false,
        debugOpen: false,
      },
    },
  });
}

/** Parse, bound, and copy an untrusted persisted DTO. Unknown fields fail. */
export function parseStoredWorkspaceSession(
  value: unknown,
): StoredWorkspaceSession {
  const record = strictObject(
    value,
    [
      "version",
      "id",
      "name",
      "createdAtUnixMs",
      "updatedAtUnixMs",
      "activeRemotes",
      "workspace",
    ],
    "workspace",
  );
  if (record.version !== WORKSPACE_SESSION_VERSION)
    invalid("workspace version is unsupported");
  const id = boundedString(record.id, 36, "workspace id");
  assertSessionId(id);
  const name = boundedString(
    record.name,
    WORKSPACE_SESSION_MAX_NAME_BYTES,
    "workspace name",
    true,
  );
  if (name !== name.trim()) invalid("workspace name is not trimmed");
  const createdAtUnixMs = timestamp(record.createdAtUnixMs, "createdAtUnixMs");
  const updatedAtUnixMs = timestamp(record.updatedAtUnixMs, "updatedAtUnixMs");
  if (updatedAtUnixMs < createdAtUnixMs)
    invalid("updatedAtUnixMs predates createdAtUnixMs");
  return deepFreeze({
    version: WORKSPACE_SESSION_VERSION,
    id,
    name,
    createdAtUnixMs,
    updatedAtUnixMs,
    activeRemotes: remotes(record.activeRemotes),
    workspace: workspace(record.workspace),
  });
}

export function isStoredWorkspaceSession(
  value: unknown,
): value is StoredWorkspaceSession {
  try {
    parseStoredWorkspaceSession(value);
    return true;
  } catch {
    return false;
  }
}

export function assertStoredWorkspaceSession(
  value: unknown,
): asserts value is StoredWorkspaceSession {
  parseStoredWorkspaceSession(value);
}

/** Validate and defensively copy a semantic patch. */
export function parseWorkspaceSessionPatch(
  value: unknown,
): WorkspaceSessionPatch {
  const patch = strictObject(
    value,
    ["name", "activeRemotes", "workspace"],
    "workspace patch",
    true,
  );
  const out: WorkspaceSessionPatch = {};
  if (has(patch, "name")) {
    const name = boundedString(
      patch.name,
      WORKSPACE_SESSION_MAX_NAME_BYTES,
      "workspace name",
      true,
    );
    if (name !== name.trim()) invalid("workspace name is not trimmed");
    out.name = name;
  }
  if (has(patch, "activeRemotes"))
    out.activeRemotes = remotes(patch.activeRemotes);
  if (has(patch, "workspace")) out.workspace = workspacePatch(patch.workspace);
  return out;
}

function workspace(value: unknown): WorkspaceSessionWorkspace {
  const input = strictObject(
    value,
    ["layout", "assignments", "focusedPaneId", "main", "panels"],
    "workspace state",
  );
  return {
    layout: layout(input.layout),
    assignments: assignments(input.assignments),
    focusedPaneId: nullableBoundedString(
      input.focusedPaneId,
      WORKSPACE_SESSION_MAX_PANE_ID_BYTES,
      "focused pane id",
    ),
    main: nullableBoundedString(
      input.main,
      WORKSPACE_SESSION_MAX_REFERENCE_BYTES,
      "main reference",
    ),
    panels: panels(input.panels),
  };
}

function workspacePatch(value: unknown): WorkspaceSessionWorkspacePatch {
  const input = strictObject(
    value,
    ["layout", "assignments", "focusedPaneId", "main", "panels"],
    "workspace state patch",
    true,
  );
  const out: WorkspaceSessionWorkspacePatch = {};
  if (has(input, "layout")) out.layout = layout(input.layout);
  if (has(input, "assignments"))
    out.assignments = assignments(input.assignments);
  if (has(input, "focusedPaneId"))
    out.focusedPaneId = nullableBoundedString(
      input.focusedPaneId,
      WORKSPACE_SESSION_MAX_PANE_ID_BYTES,
      "focused pane id",
    );
  if (has(input, "main"))
    out.main = nullableBoundedString(
      input.main,
      WORKSPACE_SESSION_MAX_REFERENCE_BYTES,
      "main reference",
    );
  if (has(input, "panels")) out.panels = panelsPatch(input.panels);
  return out;
}

function layout(value: unknown): WorkspaceSessionLayout | null {
  if (value === null) return null;
  const input = strictObject(value, ["name", "dsl"], "workspace layout");
  const dsl = boundedString(
    input.dsl,
    WORKSPACE_SESSION_MAX_LAYOUT_DSL_BYTES,
    "layout DSL",
    true,
  );
  try {
    parseDSL(dsl);
  } catch (error) {
    invalid(`layout DSL is invalid: ${asError(error).message}`);
  }
  return {
    name: boundedString(
      input.name,
      WORKSPACE_SESSION_MAX_LAYOUT_NAME_BYTES,
      "layout name",
      true,
    ),
    dsl,
  };
}

function assignments(value: unknown): Record<string, string> {
  const input = plainObject(value, "workspace assignments");
  const entries = Object.entries(input);
  if (entries.length > WORKSPACE_SESSION_MAX_ASSIGNMENTS)
    invalid("workspace assignment count exceeds its limit");
  const out: Record<string, string> = {};
  for (const [paneId, reference] of entries) {
    if (
      paneId === "__proto__" ||
      paneId === "prototype" ||
      paneId === "constructor"
    )
      invalid("assignment pane id is unsafe");
    boundedString(
      paneId,
      WORKSPACE_SESSION_MAX_PANE_ID_BYTES,
      "assignment pane id",
      true,
    );
    out[paneId] = boundedString(
      reference,
      WORKSPACE_SESSION_MAX_REFERENCE_BYTES,
      "assignment reference",
      true,
    );
  }
  return out;
}

function panels(value: unknown): WorkspaceSessionPanels {
  const input = strictObject(
    value,
    [
      "leftOpen",
      "previewOpen",
      "expandedSections",
      "project",
      "musterExpanded",
      "debugOpen",
    ],
    "workspace panels",
  );
  return {
    leftOpen: bool(input.leftOpen, "leftOpen"),
    previewOpen: bool(input.previewOpen, "previewOpen"),
    expandedSections: expandedSections(input.expandedSections),
    project: project(input.project),
    musterExpanded: bool(input.musterExpanded, "musterExpanded"),
    debugOpen: bool(input.debugOpen, "debugOpen"),
  };
}

function panelsPatch(value: unknown): WorkspaceSessionPanelsPatch {
  const input = strictObject(
    value,
    [
      "leftOpen",
      "previewOpen",
      "expandedSections",
      "project",
      "musterExpanded",
      "debugOpen",
    ],
    "workspace panels patch",
    true,
  );
  const out: WorkspaceSessionPanelsPatch = {};
  if (has(input, "leftOpen")) out.leftOpen = bool(input.leftOpen, "leftOpen");
  if (has(input, "previewOpen"))
    out.previewOpen = bool(input.previewOpen, "previewOpen");
  if (has(input, "expandedSections"))
    out.expandedSections = expandedSections(input.expandedSections);
  if (has(input, "project")) out.project = project(input.project);
  if (has(input, "musterExpanded"))
    out.musterExpanded = bool(input.musterExpanded, "musterExpanded");
  if (has(input, "debugOpen"))
    out.debugOpen = bool(input.debugOpen, "debugOpen");
  return out;
}

function project(value: unknown): WorkspaceSessionProjectSelection | null {
  if (value === null) return null;
  const base = plainObject(value, "workspace project selection");
  if (base.kind === "focused") {
    strictKeys(base, ["kind"], "focused project selection");
    return { kind: "focused" };
  }
  if (base.kind === "declared") {
    strictKeys(base, ["kind", "name"], "declared project selection");
    return {
      kind: "declared",
      name: boundedString(
        base.name,
        WORKSPACE_SESSION_MAX_PATH_BYTES,
        "declared project name",
        true,
      ),
    };
  }
  if (base.kind === "worktree") {
    strictKeys(
      base,
      ["kind", "connectionId", "path", "label"],
      "worktree project selection",
    );
    return {
      kind: "worktree",
      connectionId: boundedString(
        base.connectionId,
        WORKSPACE_SESSION_MAX_REMOTE_BYTES,
        "worktree connection id",
        true,
      ),
      path: boundedString(
        base.path,
        WORKSPACE_SESSION_MAX_PATH_BYTES,
        "worktree path",
        true,
      ),
      label: boundedString(
        base.label,
        WORKSPACE_SESSION_MAX_NAME_BYTES,
        "worktree label",
      ),
    };
  }
  invalid("workspace project selection kind is invalid");
}

function remotes(value: unknown): string[] {
  if (!Array.isArray(value)) invalid("activeRemotes is not an array");
  if (value.length > WORKSPACE_SESSION_MAX_REMOTES)
    invalid("active remote count exceeds its limit");
  const out = value.map((remote) =>
    boundedString(
      remote,
      WORKSPACE_SESSION_MAX_REMOTE_BYTES,
      "active remote",
      true,
    ),
  );
  if (out.some((remote) => remote !== remote.trim()))
    invalid("activeRemotes contains an untrimmed route name");
  if (new Set(out).size !== out.length)
    invalid("activeRemotes contains a duplicate");
  if (out.includes("local"))
    invalid('activeRemotes must not contain implicit home route "local"');
  return out;
}

function expandedSections(value: unknown): string[] {
  if (!Array.isArray(value)) invalid("expandedSections is not an array");
  if (value.length > WORKSPACE_SESSION_MAX_EXPANDED_SECTIONS)
    invalid("expanded section count exceeds its limit");
  const out = value.map((section) =>
    boundedString(
      section,
      WORKSPACE_SESSION_MAX_PANEL_ID_BYTES,
      "expanded section",
      true,
    ),
  );
  if (new Set(out).size !== out.length)
    invalid("expandedSections contains a duplicate");
  return out;
}

function timestamp(value: unknown, name: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0)
    invalid(`${name} is not a non-negative safe integer`);
  return value as number;
}

function bool(value: unknown, name: string): boolean {
  if (typeof value !== "boolean") invalid(`${name} is not a boolean`);
  return value;
}

function nullableBoundedString(
  value: unknown,
  maxBytes: number,
  name: string,
): string | null {
  return value === null ? null : boundedString(value, maxBytes, name, true);
}

function boundedString(
  value: unknown,
  maxBytes: number,
  name: string,
  nonempty = false,
): string {
  if (typeof value !== "string") invalid(`${name} is not a string`);
  if (nonempty && value.length === 0) invalid(`${name} is empty`);
  if (value.includes("\0")) invalid(`${name} contains NUL`);
  if (encoder.encode(value).length > maxBytes)
    invalid(`${name} exceeds its byte limit`);
  return value;
}

function strictObject(
  value: unknown,
  keys: readonly string[],
  name: string,
  partial = false,
): Record<string, unknown> {
  const object = plainObject(value, name);
  strictKeys(object, keys, name, partial);
  return object;
}

function plainObject(value: unknown, name: string): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value))
    invalid(`${name} is not an object`);
  const prototype = Object.getPrototypeOf(value);
  if (prototype !== Object.prototype && prototype !== null)
    invalid(`${name} does not have a plain object prototype`);
  return value as Record<string, unknown>;
}

function strictKeys(
  value: Record<string, unknown>,
  keys: readonly string[],
  name: string,
  partial = false,
): void {
  const allowed = new Set(keys);
  for (const key of Object.keys(value)) {
    if (!allowed.has(key)) invalid(`${name} has unknown field ${key}`);
  }
  if (!partial) {
    for (const key of keys) {
      if (!has(value, key)) invalid(`${name} is missing field ${key}`);
    }
  }
}

function has(value: object, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(value, key);
}

function assertSessionId(id: string): void {
  if (!isWorkspaceSessionId(id))
    invalid("workspace id is not a canonical lowercase UUID");
}

function invalid(message: string): never {
  throw new WorkspaceSessionValidationError(message);
}

export type WorkspaceSessionStoreStatus =
  | "idle"
  | "loading"
  | "ready"
  | "error"
  | "closed";

export interface WorkspaceSessionInvalidRecord {
  key: string;
  message: string;
}

export interface WorkspaceSessionStoreSnapshot {
  status: WorkspaceSessionStoreStatus;
  sessions: readonly StoredWorkspaceSession[];
  error: Error | null;
  invalidKeys: readonly string[];
  invalidRecords: readonly WorkspaceSessionInvalidRecord[];
  /** Bounded canonical IDs whose exact backend key is quarantined. */
  quarantinedSessionIds: readonly string[];
}

export type WorkspaceSessionPresence = "available" | "quarantined" | "absent";

export interface WorkspaceSessionStoreOptions {
  now?: () => number;
  randomUUID?: () => string;
  /** Number of refetch/reapply attempts after the first CAS attempt. */
  casRetries?: number;
  onInvalidRecord?: (record: WorkspaceSessionInvalidRecord) => void;
  /** @internal Deterministic seam for YAS reconnect tests. */
  yasKvFactory?: (connection: YasConnection) => WorkspaceSessionOwnedKv;
}

interface IndexedSession {
  record: StoredWorkspaceSession;
  hash: WorkspaceSessionHash;
  mtimeNs: bigint;
  byteLength: number;
  retainedBytes: number;
}

interface PendingSession {
  indexed: IndexedSession;
  previousHash: WorkspaceSessionHash | null;
}

interface SessionReconcileRequest {
  generation: number;
  kv: WorkspaceSessionKv;
  mirror: {
    readonly live: ReadonlyMap<string, WorkspaceSessionKvEntry>;
    snapshotDone: boolean;
  };
}

/**
 * One durable session catalogue over one home-server KV prefix watch.
 * Attachments are local subscriptions: they neither mutate the document nor
 * create server-visible presence. A raw YasConnection source recreates its
 * native KV facade after link loss; a fixed structural KV source must be
 * replaced by its owner if that transport permanently invalidates.
 */
export class WorkspaceSessionStore {
  private kv: WorkspaceSessionKv | null;
  private readonly yasConnection: YasConnection | null;
  private ownedKv: WorkspaceSessionOwnedKv | null = null;
  private readonly yasKvFactory: (
    connection: YasConnection,
  ) => WorkspaceSessionOwnedKv;
  private readonly now: () => number;
  private readonly randomUUID: () => string;
  private readonly casRetries: number;
  private readonly onInvalidRecord?: (
    record: WorkspaceSessionInvalidRecord,
  ) => void;
  private readonly listeners = new Set<() => void>();
  private readonly attachments = new Map<
    string,
    Set<WorkspaceSessionAttachment>
  >();
  private entries = new Map<string, IndexedSession>();
  private pending = new Map<string, PendingSession>();
  private pendingDeletes = new Map<string, WorkspaceSessionHash>();
  private invalidRecords = new Map<string, WorkspaceSessionInvalidRecord>();
  private watch: WorkspaceSessionKvWatch | null = null;
  private started = false;
  private startPromise: Promise<void> | null = null;
  private resolveInitial: (() => void) | null = null;
  private rejectInitial: ((error: Error) => void) | null = null;
  private openGeneration = 0;
  private openInFlight = false;
  private retryAttempt = 0;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  /** Serializes catalogue reconciliation with every local state mutation. */
  private stateOperationTail: Promise<void> = Promise.resolve();
  private pendingReconcile: SessionReconcileRequest | null = null;
  private reconcileQueued = false;
  private revisionValue = 0;
  private snapshot: WorkspaceSessionStoreSnapshot = freezeStoreSnapshot({
    status: "idle",
    sessions: [],
    error: null,
    invalidKeys: [],
    invalidRecords: [],
    quarantinedSessionIds: [],
  });

  constructor(
    source: WorkspaceSessionKv | YasConnection,
    options: WorkspaceSessionStoreOptions = {},
  ) {
    if (isWorkspaceSessionKv(source)) {
      this.kv = source;
      this.yasConnection = null;
    } else {
      this.kv = null;
      this.yasConnection = source;
    }
    this.now = options.now ?? Date.now;
    this.randomUUID = options.randomUUID ?? defaultRandomUUID;
    const retries = options.casRetries ?? DEFAULT_CAS_RETRIES;
    if (!Number.isInteger(retries) || retries < 0 || retries > 10)
      invalid("workspace CAS retry count is invalid");
    this.casRetries = retries;
    this.onInvalidRecord = options.onInvalidRecord;
    this.yasKvFactory =
      options.yasKvFactory ??
      ((connection) => new YasNativeWorkspaceKv(connection));
  }

  get revision(): number {
    return this.revisionValue;
  }

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  getSnapshot = (): WorkspaceSessionStoreSnapshot => this.snapshot;

  get(id: string): StoredWorkspaceSession | null {
    assertSessionId(id);
    return this.entries.get(id)?.record ?? null;
  }

  /** Exact current presence, including records omitted by bounded quarantine. */
  getPresence(id: string): WorkspaceSessionPresence {
    assertSessionId(id);
    const key = workspaceSessionKey(id);
    if (this.invalidRecords.has(key)) return "quarantined";
    const indexed = this.entries.get(id);
    const mirrored = this.watch?.mirror.live.get(key);
    if (indexed) {
      if (
        mirrored &&
        !workspaceSessionHashesEqual(mirrored.hash, indexed.hash)
      ) {
        const pending = this.pending.get(id);
        if (
          !workspaceSessionHashesEqual(pending?.indexed.hash, indexed.hash) ||
          !workspaceSessionHashesEqual(pending?.previousHash, mirrored.hash)
        )
          return "quarantined";
      }
      return "available";
    }
    return mirrored ? "quarantined" : "absent";
  }

  start(): Promise<void> {
    if (this.snapshot.status === "closed")
      return Promise.reject(new Error("Workspace store is closed"));
    if (this.snapshot.status === "ready") return Promise.resolve();
    if (this.startPromise) return this.startPromise;
    this.startPromise = new Promise<void>((resolve, reject) => {
      this.resolveInitial = resolve;
      this.rejectInitial = reject;
    });
    if (!this.started) {
      this.started = true;
      this.setStatus("loading", null);
      this.openWatch();
    } else if (!this.openInFlight && this.retryTimer === null) {
      this.setStatus("loading", this.snapshot.error);
      this.openWatch();
    }
    return this.startPromise;
  }

  close(): void {
    if (this.snapshot.status === "closed") return;
    this.started = false;
    this.openGeneration++;
    if (this.retryTimer !== null) clearTimeout(this.retryTimer);
    this.retryTimer = null;
    this.watch?.close();
    this.watch = null;
    this.ownedKv?.dispose();
    this.ownedKv = null;
    const error = new Error("Workspace store is closed");
    this.rejectInitial?.(error);
    this.resolveInitial = null;
    this.rejectInitial = null;
    this.startPromise = null;
    this.setStatus("closed", null);
    for (const handles of this.attachments.values()) {
      for (const handle of handles) handle.storeClosed();
    }
    this.attachments.clear();
  }

  dispose(): void {
    this.close();
  }

  async create(
    input: CreateWorkspaceSessionInput = {},
  ): Promise<StoredWorkspaceSession> {
    // Readiness must be established before taking the state-operation tail:
    // initial readiness itself is published by a reconciliation on that tail.
    const kv = await this.operationalKv();
    return this.enqueueStateOperation(async () => {
      const explicitId = input.id !== undefined;
      for (let attempt = 0; attempt <= this.casRetries; attempt++) {
        const id = input.id ?? this.randomUUID();
        assertSessionId(id);
        if (
          !this.entries.has(id) &&
          this.catalogueKeyCount() >= WORKSPACE_SESSION_MAX_CATALOG_ENTRIES
        )
          throw new Error("Workspace catalogue limit exceeded");
        const record = createDefaultStoredWorkspaceSession({
          id,
          name:
            input.name ??
            nextWorkspaceName(
              [...this.entries.values()].map(({ record }) => record),
            ),
          activeRemotes: input.activeRemotes,
          workspace: input.workspace,
          nowUnixMs: checkedNow(this.now()),
        });
        try {
          const bytes = encodeDocument(record);
          this.assertLocalCapacity(id, bytes.length);
          const result = await kv.kvPut(workspaceSessionKey(id), bytes, {
            create: true,
            durable: true,
          });
          this.installLocal(
            indexedSession(record, result.hash, result.mtimeNs, bytes.length),
            null,
          );
          return record;
        } catch (error) {
          if (!(error instanceof WorkspaceSessionKvConflictError) || explicitId)
            throw error;
        }
      }
      throw new Error("Could not allocate a unique workspace id");
    });
  }

  rename(id: string, name: string): Promise<StoredWorkspaceSession> {
    return this.update(id, { name });
  }

  async update(
    id: string,
    patchValue: WorkspaceSessionPatch,
  ): Promise<StoredWorkspaceSession> {
    assertSessionId(id);
    const patch = parseWorkspaceSessionPatch(patchValue);
    return this.mutateSession(id, (current) =>
      applyPatch(current, patch, this.now),
    );
  }

  async setRemoteActive(
    id: string,
    remoteName: string,
    active: boolean,
  ): Promise<StoredWorkspaceSession> {
    assertSessionId(id);
    if (typeof active !== "boolean")
      throw new WorkspaceSessionValidationError(
        "workspace remote membership must be boolean",
      );
    const parsed = parseWorkspaceSessionPatch({
      activeRemotes: [remoteName],
    }).activeRemotes!;
    const name = parsed[0]!;
    return this.mutateSession(id, (current) => {
      const alreadyActive = current.activeRemotes.includes(name);
      if (alreadyActive === active) return current;
      const activeRemotes = active
        ? [...current.activeRemotes, name]
        : current.activeRemotes.filter((candidate) => candidate !== name);
      return applyPatch(current, { activeRemotes }, this.now);
    });
  }

  private async mutateSession(
    id: string,
    mutate: (current: StoredWorkspaceSession) => StoredWorkspaceSession,
  ): Promise<StoredWorkspaceSession> {
    assertSessionId(id);
    const kv = await this.operationalKv();
    return this.enqueueStateOperation(async () => {
      let lastConflict: WorkspaceSessionKvConflictError | null = null;
      for (let attempt = 0; attempt <= this.casRetries; attempt++) {
        const current = await this.fetchIndexed(kv, id);
        if (!current) throw new WorkspaceSessionNotFoundError(id);
        const next = mutate(current.record);
        if (sameRecord(current.record, next)) return current.record;
        try {
          const bytes = encodeDocument(next);
          this.assertInstallCapacity(id, bytes.length);
          const result = await kv.kvPut(workspaceSessionKey(id), bytes, {
            ifHash: current.hash,
            durable: true,
          });
          this.installLocal(
            indexedSession(next, result.hash, result.mtimeNs, bytes.length),
            current.hash,
          );
          return next;
        } catch (error) {
          if (!(error instanceof WorkspaceSessionKvConflictError)) throw error;
          lastConflict = error;
        }
      }
      throw lastConflict ?? new Error("Workspace update conflicted");
    });
  }

  async delete(id: string): Promise<void> {
    assertSessionId(id);
    const kv = await this.operationalKv();
    return this.enqueueStateOperation(async () => {
      let lastConflict: WorkspaceSessionKvConflictError | null = null;
      for (let attempt = 0; attempt <= this.casRetries; attempt++) {
        const current = await kv.kvFetch(workspaceSessionKey(id));
        if (!current) {
          const previousHash = this.entries.get(id)?.hash;
          this.pending.delete(id);
          if (previousHash !== undefined)
            this.pendingDeletes.set(id, previousHash);
          this.removeLocal(id, false);
          return;
        }
        try {
          await kv.kvDelete(workspaceSessionKey(id), {
            ifHash: current.hash,
            durable: true,
          });
          this.pending.delete(id);
          this.pendingDeletes.set(id, current.hash);
          this.removeLocal(id, false);
          return;
        } catch (error) {
          if (!(error instanceof WorkspaceSessionKvConflictError)) throw error;
          lastConflict = error;
        }
      }
      throw lastConflict ?? new Error("Workspace delete conflicted");
    });
  }

  async attach(id: string): Promise<WorkspaceSessionAttachment> {
    assertSessionId(id);
    const kv = await this.operationalKv();
    return this.enqueueStateOperation(async () => {
      const previous = this.entries.get(id) ?? null;
      const key = workspaceSessionKey(id);
      let indexed: IndexedSession | null;
      let usingLastGood = false;
      try {
        indexed = await this.fetchIndexed(kv, id);
      } catch (error) {
        if (!(error instanceof WorkspaceSessionValidationError)) throw error;
        this.quarantineLocal(key, error.message);
        if (!previous) throw error;
        indexed = previous;
        usingLastGood = true;
      }
      if (!indexed) {
        this.pending.delete(id);
        if (previous) this.pendingDeletes.set(id, previous.hash);
        this.removeLocal(id, false);
        throw new WorkspaceSessionNotFoundError(id);
      }
      if (
        !usingLastGood &&
        !workspaceSessionHashesEqual(previous?.hash, indexed.hash)
      ) {
        try {
          this.assertInstallCapacity(id, indexed.byteLength);
          this.installLocal(indexed, previous?.hash ?? null);
        } catch (error) {
          if (!(error instanceof WorkspaceSessionValidationError)) throw error;
          this.quarantineLocal(key, error.message);
          if (!previous) throw error;
          indexed = previous;
        }
      } else if (!usingLastGood) {
        this.clearLocalQuarantine(key);
        indexed = previous ?? indexed;
      }
      const attachment = new WorkspaceSessionAttachment(this, indexed.record);
      let handles = this.attachments.get(id);
      if (!handles) this.attachments.set(id, (handles = new Set()));
      handles.add(attachment);
      return attachment;
    });
  }

  /** @internal */
  releaseAttachment(attachment: WorkspaceSessionAttachment): void {
    const handles = this.attachments.get(attachment.id);
    if (!handles) return;
    handles.delete(attachment);
    if (handles.size === 0) this.attachments.delete(attachment.id);
  }

  private async operationalKv(): Promise<WorkspaceSessionKv> {
    await this.start();
    if (this.snapshot.status !== "ready") await this.waitUntilReady();
    if (!this.kv) throw new Error("Workspace KV is unavailable");
    return this.kv;
  }

  private enqueueStateOperation<T>(operation: () => Promise<T>): Promise<T> {
    const result = this.stateOperationTail.then(operation);
    this.stateOperationTail = result.then(
      () => undefined,
      () => undefined,
    );
    return result;
  }

  private waitUntilReady(): Promise<void> {
    if (this.snapshot.status === "ready") return Promise.resolve();
    if (this.snapshot.status === "error" || this.snapshot.status === "closed")
      return Promise.reject(
        this.snapshot.error ?? new Error("Workspace store is closed"),
      );
    return new Promise((resolve, reject) => {
      const unsubscribe = this.subscribe(() => {
        if (this.snapshot.status === "ready") {
          unsubscribe();
          resolve();
        } else if (
          this.snapshot.status === "error" ||
          this.snapshot.status === "closed"
        ) {
          unsubscribe();
          reject(this.snapshot.error ?? new Error("Workspace store is closed"));
        }
      });
    });
  }

  private openWatch(): void {
    if (!this.started || this.openInFlight || this.retryTimer !== null) return;
    this.openInFlight = true;
    const generation = ++this.openGeneration;
    let kv: WorkspaceSessionKv;
    try {
      if (this.yasConnection) {
        this.ownedKv?.dispose();
        this.ownedKv = this.yasKvFactory(this.yasConnection);
        this.kv = this.ownedKv;
      }
      if (!this.kv) throw new Error("Workspace KV is unavailable");
      kv = this.kv;
    } catch (error) {
      this.openInFlight = false;
      this.watchFailed(generation, asError(error));
      return;
    }
    void kv
      .watchKv(WORKSPACE_SESSION_KEY_PREFIX, {
        inlineMax: WATCH_INLINE_MAX,
        onUpdate: (mirror) => this.enqueueReconcile(generation, kv, mirror),
        onClosed: (error) => this.watchFailed(generation, error),
      })
      .then((watch) => {
        if (!this.started || generation !== this.openGeneration) {
          watch.close();
          return;
        }
        this.openInFlight = false;
        this.watch = watch;
        this.enqueueReconcile(generation, kv, watch.mirror);
      })
      .catch((error: unknown) => this.watchFailed(generation, asError(error)));
  }

  private watchFailed(generation: number, error: Error): void {
    if (!this.started || generation !== this.openGeneration) return;
    this.openGeneration++;
    this.openInFlight = false;
    this.watch?.close();
    this.watch = null;
    this.rejectStart(error);
    if (!this.yasConnection) {
      this.started = false;
      this.fail(error);
      return;
    }
    this.ownedKv?.dispose();
    this.ownedKv = null;
    this.kv = null;
    this.setStatus("loading", error);
    const delay = Math.min(5_000, 100 * 2 ** Math.min(this.retryAttempt++, 6));
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null;
      this.openWatch();
    }, delay);
  }

  private enqueueReconcile(
    generation: number,
    kv: WorkspaceSessionKv,
    mirror: {
      readonly live: ReadonlyMap<string, WorkspaceSessionKvEntry>;
      snapshotDone: boolean;
    },
  ): void {
    if (!this.started || generation !== this.openGeneration) return;
    this.pendingReconcile = { generation, kv, mirror };
    this.scheduleReconcile();
  }

  private scheduleReconcile(): void {
    if (this.reconcileQueued || !this.pendingReconcile) return;
    this.reconcileQueued = true;
    void this.enqueueStateOperation(async () => {
      const request = this.pendingReconcile;
      this.pendingReconcile = null;
      if (!request) return;
      try {
        await this.reconcile(request.generation, request.kv, request.mirror);
      } catch (error) {
        this.watchFailed(request.generation, asError(error));
      }
    }).finally(() => {
      this.reconcileQueued = false;
      // Updates observed while the previous reconciliation was running get one
      // fresh position at the tail, behind mutations queued in the meantime.
      if (this.pendingReconcile) this.scheduleReconcile();
    });
  }

  private async reconcile(
    generation: number,
    kv: WorkspaceSessionKv,
    mirror: {
      readonly live: ReadonlyMap<string, WorkspaceSessionKvEntry>;
      snapshotDone: boolean;
    },
  ): Promise<void> {
    if (!this.started || generation !== this.openGeneration) return;
    // Native YAS snapshot construction is transactional.
    // Never replace the last good catalogue with a partial recovery snapshot.
    if (!mirror.snapshotDone) return;
    const records = [...mirror.live.entries()]
      .filter(([key]) => key.startsWith(WORKSPACE_SESSION_KEY_PREFIX))
      .sort(([a], [b]) => a.localeCompare(b));
    const admittedMirrorKeys = new Set(
      records
        .slice(0, WORKSPACE_SESSION_MAX_CATALOG_ENTRIES)
        .map(([key]) => key),
    );
    const next = new Map<string, IndexedSession>();
    const quarantined = new Map<string, WorkspaceSessionInvalidRecord>();
    let retainedBytes = 0;
    const preserveLastGood = (id: string): void => {
      const current = this.entries.get(id);
      if (
        !current ||
        next.has(id) ||
        next.size >= WORKSPACE_SESSION_MAX_CATALOG_ENTRIES ||
        retainedBytes + current.retainedBytes >
          WORKSPACE_SESSION_MAX_RETAINED_BYTES
      )
        return;
      retainedBytes += current.retainedBytes;
      next.set(id, current);
    };
    for (let index = 0; index < records.length; index++) {
      const [key, entry] = records[index];
      if (index >= WORKSPACE_SESSION_MAX_CATALOG_ENTRIES) {
        quarantine(quarantined, key, "workspace catalogue limit exceeded");
        continue;
      }
      const id = key.slice(WORKSPACE_SESSION_KEY_PREFIX.length);
      if (!isWorkspaceSessionId(id)) {
        quarantine(quarantined, key, "workspace key suffix is not a UUID");
        continue;
      }
      const deletedHash = this.pendingDeletes.get(id);
      if (deletedHash !== undefined) {
        if (workspaceSessionHashesEqual(entry.hash, deletedHash)) continue;
        this.pendingDeletes.delete(id);
      }
      const pending = this.pending.get(id);
      if (pending) {
        if (workspaceSessionHashesEqual(entry.hash, pending.indexed.hash)) {
          this.pending.delete(id);
          if (
            retainedBytes + pending.indexed.retainedBytes >
            WORKSPACE_SESSION_MAX_RETAINED_BYTES
          ) {
            quarantine(
              quarantined,
              key,
              "workspace retained-byte limit exceeded",
            );
            continue;
          }
          retainedBytes += pending.indexed.retainedBytes;
          next.set(id, pending.indexed);
          continue;
        }
        if (
          pending.previousHash !== null &&
          workspaceSessionHashesEqual(entry.hash, pending.previousHash)
        ) {
          if (
            retainedBytes + pending.indexed.retainedBytes >
            WORKSPACE_SESSION_MAX_RETAINED_BYTES
          ) {
            quarantine(
              quarantined,
              key,
              "workspace retained-byte limit exceeded",
            );
            continue;
          }
          retainedBytes += pending.indexed.retainedBytes;
          next.set(id, pending.indexed);
          continue;
        }
        this.pending.delete(id);
      }
      try {
        if (
          retainedBytes + retainedEstimate(entry.size) >
          WORKSPACE_SESSION_MAX_RETAINED_BYTES
        ) {
          quarantine(
            quarantined,
            key,
            "workspace retained-byte limit exceeded",
          );
          preserveLastGood(id);
          continue;
        }
        const loaded = await this.loadMirrorEntry(kv, key, id, entry);
        if (!loaded) continue;
        if (
          retainedBytes + loaded.indexed.retainedBytes >
          WORKSPACE_SESSION_MAX_RETAINED_BYTES
        ) {
          quarantine(
            quarantined,
            key,
            "workspace retained-byte limit exceeded",
          );
          preserveLastGood(id);
          continue;
        }
        retainedBytes += loaded.indexed.retainedBytes;
        const current = this.entries.get(id);
        next.set(
          id,
          current &&
            workspaceSessionHashesEqual(current.hash, loaded.indexed.hash)
            ? current
            : loaded.indexed,
        );
        if (loaded.previousHash !== undefined)
          this.pending.set(id, {
            indexed: loaded.indexed,
            previousHash: loaded.previousHash,
          });
      } catch (error) {
        quarantine(quarantined, key, asError(error).message);
        preserveLastGood(id);
      }
    }
    for (const [id, pendingValue] of [...this.pending]) {
      let pending = pendingValue;
      if (pending.previousHash !== null || next.has(id)) continue;
      const key = workspaceSessionKey(id);
      // A complete mirror can legitimately lag a just-completed create, but an
      // intervening client can also delete that key before either watch update
      // is reconciled. Confirm mirror absence authoritatively instead of
      // retaining an optimistic create forever.
      if (!mirror.live.has(key)) {
        try {
          const fetched = await this.fetchIndexed(kv, id);
          if (!fetched) {
            this.pending.delete(id);
            continue;
          }
          if (
            !workspaceSessionHashesEqual(fetched.hash, pending.indexed.hash)
          ) {
            pending = { indexed: fetched, previousHash: null };
            this.pending.set(id, pending);
          }
        } catch (error) {
          if (!(error instanceof WorkspaceSessionValidationError)) throw error;
          quarantine(quarantined, key, error.message);
        }
      }
      if (
        next.size >= WORKSPACE_SESSION_MAX_CATALOG_ENTRIES ||
        (!admittedMirrorKeys.has(key) &&
          records.length >= WORKSPACE_SESSION_MAX_CATALOG_ENTRIES)
      ) {
        this.pending.delete(id);
        quarantine(quarantined, key, "workspace catalogue limit exceeded");
        continue;
      }
      if (
        retainedBytes + pending.indexed.retainedBytes >
        WORKSPACE_SESSION_MAX_RETAINED_BYTES
      ) {
        this.pending.delete(id);
        quarantine(quarantined, key, "workspace retained-byte limit exceeded");
        continue;
      }
      retainedBytes += pending.indexed.retainedBytes;
      next.set(id, pending.indexed);
    }
    // Optimistic records excluded from the bounded next catalogue must not be
    // retained out-of-band in `pending` after their visible entry is removed.
    for (const id of this.pending.keys()) {
      if (!next.has(id)) this.pending.delete(id);
    }
    for (const id of this.pendingDeletes.keys()) {
      if (!mirror.live.has(workspaceSessionKey(id)))
        this.pendingDeletes.delete(id);
    }
    if (!this.started || generation !== this.openGeneration) return;
    this.commitCatalog(next, quarantined);
    if (mirror.snapshotDone) {
      this.retryAttempt = 0;
      this.setStatus(
        "ready",
        quarantined.size === 0
          ? null
          : new WorkspaceSessionValidationError(
              `${quarantined.size} workspace record(s) quarantined`,
            ),
      );
      this.resolveInitial?.();
      this.resolveInitial = null;
      this.rejectInitial = null;
      this.startPromise = null;
    }
  }

  private async loadMirrorEntry(
    kv: WorkspaceSessionKv,
    key: string,
    id: string,
    entry: WorkspaceSessionKvEntry,
  ): Promise<{
    indexed: IndexedSession;
    previousHash?: WorkspaceSessionHash;
  } | null> {
    if (entry.size > WORKSPACE_SESSION_MAX_DOCUMENT_BYTES)
      invalid("workspace document exceeds its byte limit");
    let bytes: Uint8Array;
    let hash = entry.hash;
    let previousHash: WorkspaceSessionHash | undefined;
    if (entry.value !== null) bytes = new Uint8Array(entry.value);
    else {
      const fetched = await kv.kvFetch(key);
      if (!fetched) return null;
      bytes = new Uint8Array(fetched.value);
      hash = fetched.hash;
      if (!workspaceSessionHashesEqual(hash, entry.hash))
        previousHash = copyWorkspaceSessionHash(entry.hash);
    }
    const record = decodeDocument(bytes, id);
    return {
      indexed: indexedSession(
        record,
        hash,
        entry.mtimeNs,
        Math.max(entry.size, bytes.length),
      ),
      previousHash,
    };
  }

  private async fetchIndexed(
    kv: WorkspaceSessionKv,
    id: string,
  ): Promise<IndexedSession | null> {
    const fetched = await kv.kvFetch(workspaceSessionKey(id));
    if (!fetched) return null;
    const record = decodeDocument(new Uint8Array(fetched.value), id);
    const mirrorEntry = this.watch?.mirror.live.get(workspaceSessionKey(id));
    const mirroredSize =
      mirrorEntry && workspaceSessionHashesEqual(mirrorEntry.hash, fetched.hash)
        ? mirrorEntry.size
        : fetched.value.length;
    if (mirroredSize > WORKSPACE_SESSION_MAX_DOCUMENT_BYTES)
      invalid("workspace document exceeds its byte limit");
    return indexedSession(
      record,
      fetched.hash,
      mirrorEntry && workspaceSessionHashesEqual(mirrorEntry.hash, fetched.hash)
        ? mirrorEntry.mtimeNs
        : (this.entries.get(id)?.mtimeNs ?? 0n),
      Math.max(mirroredSize, fetched.value.length),
    );
  }

  private installLocal(
    indexed: IndexedSession,
    previousHash: WorkspaceSessionHash | null,
  ): void {
    const id = indexed.record.id;
    this.pendingDeletes.delete(id);
    this.pending.set(id, { indexed, previousHash });
    const next = new Map(this.entries);
    next.set(id, indexed);
    const invalid = new Map(this.invalidRecords);
    invalid.delete(workspaceSessionKey(id));
    this.commitCatalog(next, invalid);
    this.updateReadyQuarantineError();
  }

  private removeLocal(id: string, clearPending = true): void {
    if (clearPending) {
      this.pending.delete(id);
      this.pendingDeletes.delete(id);
    }
    const key = workspaceSessionKey(id);
    if (!this.entries.has(id) && !this.invalidRecords.has(key)) return;
    const next = new Map(this.entries);
    next.delete(id);
    const invalid = new Map(this.invalidRecords);
    invalid.delete(key);
    this.commitCatalog(next, invalid);
    this.updateReadyQuarantineError();
  }

  private catalogueKeyCount(): number {
    const mirrored = this.watch
      ? [...this.watch.mirror.live.keys()].filter((key) =>
          key.startsWith(WORKSPACE_SESSION_KEY_PREFIX),
        ).length
      : 0;
    const localKeys = new Set(this.invalidRecords.keys());
    for (const id of this.entries.keys())
      localKeys.add(workspaceSessionKey(id));
    return Math.max(mirrored, localKeys.size);
  }

  private assertInstallCapacity(id: string, byteLength: number): void {
    if (!this.entries.has(id)) {
      const key = workspaceSessionKey(id);
      const known =
        this.invalidRecords.has(key) ||
        this.watch?.mirror.live.has(key) === true;
      const count = this.catalogueKeyCount();
      if (
        count > WORKSPACE_SESSION_MAX_CATALOG_ENTRIES ||
        (!known && count >= WORKSPACE_SESSION_MAX_CATALOG_ENTRIES)
      )
        invalid("workspace catalogue limit exceeded");
    }
    this.assertLocalCapacity(id, byteLength);
  }

  private assertLocalCapacity(id: string, byteLength: number): void {
    const retained = [...this.entries.entries()].reduce(
      (total, [entryId, value]) =>
        total + (entryId === id ? 0 : value.retainedBytes),
      0,
    );
    if (
      retained + retainedEstimate(byteLength) >
      WORKSPACE_SESSION_MAX_RETAINED_BYTES
    )
      invalid("workspace retained-byte limit exceeded");
  }

  private quarantineLocal(key: string, message: string): void {
    const invalidNext = new Map(this.invalidRecords);
    quarantine(invalidNext, key, message);
    this.commitCatalog(new Map(this.entries), invalidNext);
    this.updateReadyQuarantineError();
  }

  private clearLocalQuarantine(key: string): void {
    if (!this.invalidRecords.has(key)) return;
    const invalidNext = new Map(this.invalidRecords);
    invalidNext.delete(key);
    this.commitCatalog(new Map(this.entries), invalidNext);
    this.updateReadyQuarantineError();
  }

  private updateReadyQuarantineError(): void {
    if (this.snapshot.status !== "ready") return;
    this.setStatus(
      "ready",
      this.invalidRecords.size === 0
        ? null
        : new WorkspaceSessionValidationError(
            `${this.invalidRecords.size} workspace record(s) quarantined`,
          ),
    );
  }

  private commitCatalog(
    next: Map<string, IndexedSession>,
    invalidNext: Map<string, WorkspaceSessionInvalidRecord>,
  ): void {
    const changed = new Set<string>();
    for (const [id, value] of this.entries) {
      if (!workspaceSessionHashesEqual(next.get(id)?.hash, value.hash))
        changed.add(id);
    }
    for (const [id, value] of next) {
      if (!workspaceSessionHashesEqual(this.entries.get(id)?.hash, value.hash))
        changed.add(id);
    }
    const invalidChanged = !sameInvalid(this.invalidRecords, invalidNext);
    if (changed.size === 0 && !invalidChanged) return;
    const previousInvalid = this.invalidRecords;
    this.entries = next;
    this.invalidRecords = invalidNext;
    this.publish();
    for (const id of changed) {
      const handles = this.attachments.get(id);
      if (!handles) continue;
      const record = this.entries.get(id)?.record;
      if (record) {
        for (const attachment of handles) attachment.replace(record);
      } else {
        this.attachments.delete(id);
        for (const attachment of handles) attachment.recordDeleted();
      }
    }
    for (const [key, record] of invalidNext) {
      if (previousInvalid.get(key)?.message !== record.message)
        this.onInvalidRecord?.(record);
    }
  }

  private fail(error: Error): void {
    this.setStatus("error", error);
    this.rejectStart(error);
  }

  private rejectStart(error: Error): void {
    this.rejectInitial?.(error);
    this.resolveInitial = null;
    this.rejectInitial = null;
    this.startPromise = null;
  }

  private setStatus(
    status: WorkspaceSessionStoreStatus,
    error: Error | null,
  ): void {
    if (this.snapshot.status === status && this.snapshot.error === error)
      return;
    this.snapshot = freezeStoreSnapshot({ ...this.snapshot, status, error });
    this.emit();
  }

  private publish(): void {
    const sessions = [...this.entries.values()]
      .map(({ record }) => record)
      .sort(
        (a, b) =>
          b.updatedAtUnixMs - a.updatedAtUnixMs || a.id.localeCompare(b.id),
      );
    const invalidRecords = [...this.invalidRecords.values()].sort((a, b) =>
      a.key.localeCompare(b.key),
    );
    const quarantinedSessionIds = invalidRecords
      .map(({ key }) => workspaceSessionIdFromKey(key))
      .filter((id): id is string => id !== null);
    this.snapshot = freezeStoreSnapshot({
      ...this.snapshot,
      sessions,
      invalidKeys: invalidRecords.map(({ key }) => key),
      invalidRecords,
      quarantinedSessionIds,
    });
    this.emit();
  }

  private emit(): void {
    this.revisionValue++;
    for (const listener of [...this.listeners]) listener();
  }
}

export class WorkspaceSessionAttachment {
  private readonly listeners = new Set<() => void>();
  private current: StoredWorkspaceSession | null;
  private attached = true;
  private revisionValue = 0;

  constructor(
    private readonly store: WorkspaceSessionStore,
    initial: StoredWorkspaceSession,
  ) {
    this.id = initial.id;
    this.current = initial;
  }

  readonly id: string;

  get revision(): number {
    return this.revisionValue;
  }

  get detached(): boolean {
    return !this.attached;
  }

  subscribe = (listener: () => void): (() => void) => {
    if (!this.attached) return () => undefined;
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  getSnapshot = (): StoredWorkspaceSession | null => this.current;

  update(patch: WorkspaceSessionPatch): Promise<StoredWorkspaceSession> {
    this.assertAttached();
    return this.store.update(this.id, patch);
  }

  rename(name: string): Promise<StoredWorkspaceSession> {
    this.assertAttached();
    return this.store.rename(this.id, name);
  }

  setRemoteActive(
    remoteName: string,
    active: boolean,
  ): Promise<StoredWorkspaceSession> {
    this.assertAttached();
    return this.store.setRemoteActive(this.id, remoteName, active);
  }

  delete(): Promise<void> {
    this.assertAttached();
    return this.store.delete(this.id);
  }

  detach(): void {
    if (!this.attached) return;
    this.attached = false;
    this.store.releaseAttachment(this);
    this.current = null;
    this.notify();
    this.listeners.clear();
  }

  /** @internal */
  replace(value: StoredWorkspaceSession | null): void {
    if (!this.attached || this.current === value) return;
    this.current = value;
    this.notify();
  }

  /** @internal */
  storeClosed(): void {
    if (!this.attached) return;
    this.attached = false;
    this.current = null;
    this.notify();
    this.listeners.clear();
  }

  /** @internal */
  recordDeleted(): void {
    if (!this.attached) return;
    this.attached = false;
    this.current = null;
    this.notify();
    this.listeners.clear();
  }

  private assertAttached(): void {
    if (!this.attached) throw new Error("Workspace is detached");
  }

  private notify(): void {
    this.revisionValue++;
    for (const listener of [...this.listeners]) listener();
  }
}

function applyPatch(
  current: StoredWorkspaceSession,
  patch: WorkspaceSessionPatch,
  now: () => number,
): StoredWorkspaceSession {
  const workspacePatchValue = patch.workspace;
  const panelsPatchValue = workspacePatchValue?.panels;
  const next = parseStoredWorkspaceSession({
    ...current,
    name: patch.name ?? current.name,
    activeRemotes: patch.activeRemotes ?? current.activeRemotes,
    workspace: workspacePatchValue
      ? {
          ...current.workspace,
          ...(has(workspacePatchValue, "layout")
            ? { layout: workspacePatchValue.layout }
            : {}),
          ...(has(workspacePatchValue, "assignments")
            ? { assignments: workspacePatchValue.assignments }
            : {}),
          ...(has(workspacePatchValue, "focusedPaneId")
            ? { focusedPaneId: workspacePatchValue.focusedPaneId }
            : {}),
          ...(has(workspacePatchValue, "main")
            ? { main: workspacePatchValue.main }
            : {}),
          panels: panelsPatchValue
            ? { ...current.workspace.panels, ...panelsPatchValue }
            : current.workspace.panels,
        }
      : current.workspace,
    updatedAtUnixMs: current.updatedAtUnixMs,
  });
  if (sameRecord(current, next)) return current;
  const updatedAtUnixMs = Math.max(
    checkedNow(now()),
    current.updatedAtUnixMs + 1,
  );
  if (!Number.isSafeInteger(updatedAtUnixMs))
    invalid("workspace updated timestamp overflowed");
  return parseStoredWorkspaceSession({ ...next, updatedAtUnixMs });
}

function sameRecord(
  left: StoredWorkspaceSession,
  right: StoredWorkspaceSession,
): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

function retainedEstimate(byteLength: number): number {
  if (!Number.isSafeInteger(byteLength) || byteLength < 0)
    invalid("workspace byte length is invalid");
  // UTF-16 strings, parsed object/array entries, and a bounded bookkeeping
  // allowance. Deliberately conservative relative to the encoded JSON.
  return byteLength * 4 + 4_096;
}

function indexedSession(
  record: StoredWorkspaceSession,
  hash: WorkspaceSessionHash,
  mtimeNs: bigint,
  byteLength: number,
): IndexedSession {
  return {
    record,
    hash: copyWorkspaceSessionHash(hash),
    mtimeNs,
    byteLength,
    retainedBytes: retainedEstimate(byteLength),
  };
}

function encodeDocument(record: StoredWorkspaceSession): Uint8Array {
  const canonical = parseStoredWorkspaceSession(record);
  const bytes = encoder.encode(JSON.stringify(canonical));
  if (bytes.length > WORKSPACE_SESSION_MAX_DOCUMENT_BYTES)
    invalid("workspace document exceeds its byte limit");
  return bytes;
}

function decodeDocument(
  bytesValue: Uint8Array,
  expectedId: string,
): StoredWorkspaceSession {
  const bytes = new Uint8Array(bytesValue);
  if (bytes.length > WORKSPACE_SESSION_MAX_DOCUMENT_BYTES)
    invalid("workspace document exceeds its byte limit");
  let parsed: unknown;
  try {
    parsed = JSON.parse(decoder.decode(bytes));
  } catch (error) {
    throw new WorkspaceSessionValidationError(
      `workspace JSON is invalid: ${asError(error).message}`,
    );
  }
  const record = parseStoredWorkspaceSession(parsed);
  if (record.id !== expectedId)
    invalid("workspace key and document id do not match");
  return record;
}

function checkedNow(value: number): number {
  return timestamp(value, "current time");
}

function defaultRandomUUID(): string {
  const randomUUID = globalThis.crypto?.randomUUID;
  if (!randomUUID)
    throw new Error("crypto.randomUUID is unavailable for workspace creation");
  return randomUUID.call(globalThis.crypto);
}

function isWorkspaceSessionKv(value: unknown): value is WorkspaceSessionKv {
  if (value === null || typeof value !== "object") return false;
  const candidate = value as Partial<WorkspaceSessionKv>;
  return (
    typeof candidate.kvPut === "function" &&
    typeof candidate.kvDelete === "function" &&
    typeof candidate.kvFetch === "function" &&
    typeof candidate.watchKv === "function"
  );
}

function quarantine(
  target: Map<string, WorkspaceSessionInvalidRecord>,
  key: string,
  message: string,
): void {
  if (!target.has(key) && target.size >= WORKSPACE_SESSION_MAX_CATALOG_ENTRIES)
    return;
  target.set(key, { key, message });
}

function sameInvalid(
  left: ReadonlyMap<string, WorkspaceSessionInvalidRecord>,
  right: ReadonlyMap<string, WorkspaceSessionInvalidRecord>,
): boolean {
  if (left.size !== right.size) return false;
  for (const [key, record] of left) {
    if (right.get(key)?.message !== record.message) return false;
  }
  return true;
}

function asError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

function deepFreeze<T>(value: T): T {
  if (value === null || typeof value !== "object" || Object.isFrozen(value))
    return value;
  for (const child of Object.values(value as Record<string, unknown>))
    deepFreeze(child);
  return Object.freeze(value) as T;
}

function freezeStoreSnapshot(
  value: WorkspaceSessionStoreSnapshot,
): WorkspaceSessionStoreSnapshot {
  for (const record of value.invalidRecords) Object.freeze(record);
  Object.freeze(value.sessions);
  Object.freeze(value.invalidKeys);
  Object.freeze(value.invalidRecords);
  Object.freeze(value.quarantinedSessionIds);
  return Object.freeze(value);
}
