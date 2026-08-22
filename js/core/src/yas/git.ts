/** YAS Git family v1 codecs and browser client. */

import * as g from "./generated";
import type { YasConnection, YasReceiveBudgetLease } from "./session";
import { decodeFsPath, encodeFsPath, type YasFsPath } from "./fs";
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
  YasStateCatalogueRetention,
  YasStateSubscription,
  decodeWatchResult,
  decodeStateEvent,
  encodeWatch,
  estimateStateRetainedBytes,
  type YasStateBatch,
  type YasWatchOptions,
} from "./state";
import {
  YAS_TRANSFER_MODE_BYTE,
  YAS_TRANSFER_MODE_MESSAGE,
  YAS_TRANSFER_SENDER_TO_RECEIVER,
  decodeTransferDescriptor,
  encodeTransferDescriptor,
  transfersFor,
  type YasTransfer,
  type YasTransferDescriptor,
} from "./transfer";
import {
  YasCursor,
  YasProtocolError,
  YasWriter,
  decodeExtensions,
  encodeExtensions,
  encodeTypedRecord,
  type YasExtension,
  type YasTypedRecord,
} from "./wire";

export {
  YAS_FAMILY_GIT,
  YAS_GIT_CLOSE,
  YAS_GIT_CLOSED,
  YAS_GIT_FETCH,
  YAS_GIT_OPEN,
  YAS_GIT_PROGRESS,
  YAS_GIT_QUERY,
  YAS_GIT_QUERY_STATE,
  YAS_GIT_QUERY_STATE_ACK,
  YAS_GIT_STATE,
  YAS_GIT_STATE_ACK,
  YAS_GIT_UNWATCH,
  YAS_GIT_UNWATCH_QUERY,
  YAS_GIT_VERSION,
  YAS_GIT_WATCH,
  YAS_GIT_WATCH_QUERY,
} from "./generated";

export interface YasGitObjectId {
  algorithm: number;
  bytes: Uint8Array;
}

export type YasGitRepositorySource =
  | { kind: "platform-path"; path: Uint8Array }
  | { kind: "fs"; rootHandle: bigint; path: YasFsPath }
  | { kind: "submodule"; parentRepository: bigint; path: YasFsPath }
  | { kind: "terminal-cwd"; terminalHandle: bigint; suffix: YasFsPath };

export interface YasGitOpen {
  source: YasGitRepositorySource;
  extensions?: readonly YasExtension[];
}

export interface YasGitOpenResult {
  repositoryHandle: bigint;
  repositoryRevision: bigint;
  objectAlgorithm: number;
  repositoryFlags: number;
  canonicalWorktreePath: Uint8Array;
  canonicalGitDir: Uint8Array;
  extensions: readonly YasExtension[];
}

export interface YasGitWatchOptions {
  refsSettleMs?: number;
  statusSettleMs?: number;
  refPrefixes?: readonly Uint8Array[];
}

export type YasGitQueryEndpoint =
  | { kind: "empty" }
  | { kind: "commit"; object: YasGitObjectId }
  | { kind: "tree"; object: YasGitObjectId }
  | { kind: "index" }
  | { kind: "worktree" }
  | { kind: "merge-base"; object: YasGitObjectId };

export type YasGitQueryCursor =
  | { kind: "start" }
  | { kind: "log-frontier"; objects: readonly YasGitObjectId[] }
  | { kind: "path"; path: YasFsPath }
  | { kind: "platform-path"; path: Uint8Array }
  | { kind: "patch"; path: YasFsPath; position: bigint }
  | { kind: "position"; position: bigint };

export type YasGitQueryBody =
  | { kind: "resolve"; spec: Uint8Array }
  | { kind: "merge-base"; objects: readonly YasGitObjectId[] }
  | {
      kind: "log";
      spec: Uint8Array;
      tips: readonly YasGitObjectId[];
      hides: readonly YasGitObjectId[];
      path?: YasFsPath;
      flags: number;
    }
  | { kind: "tree"; tree: YasGitObjectId; path: YasFsPath }
  | {
      kind: "blob";
      object: YasGitObjectId;
      path?: YasFsPath;
      offset: bigint;
      maxBytes: number;
      flags: number;
    }
  | {
      kind: "diff";
      left: YasGitQueryEndpoint;
      right: YasGitQueryEndpoint;
      path?: YasFsPath;
      renameThreshold: number;
      flags: number;
    }
  | {
      kind: "patch";
      left: YasGitQueryEndpoint;
      right: YasGitQueryEndpoint;
      path?: YasFsPath;
      contextLines: number;
      renameThreshold: number;
      maxBytes: number;
      flags: number;
    }
  | { kind: "index"; path?: YasFsPath; flags: number }
  | {
      kind: "discover";
      source: YasGitRepositorySource;
      maxDepth: number;
      flags: number;
    }
  | {
      kind: "blame";
      object: YasGitObjectId;
      path: YasFsPath;
      startLine: number;
      lineCount: number;
      flags: number;
    }
  | { kind: "reflog"; name: Uint8Array; flags: number }
  | { kind: "worktrees" };

export interface YasGitQuery {
  repositoryHandle: bigint;
  maxRecords: number;
  cursor: YasGitQueryCursor;
  initialReceiveCredit: bigint;
  body: YasGitQueryBody;
  extensions?: readonly YasExtension[];
}

export interface YasGitCommitRecord {
  kind: "commit";
  flags: number;
  object: YasGitObjectId;
  tree: YasGitObjectId;
  parents: readonly YasGitObjectId[];
  authoredUnixSeconds: bigint;
  authorTimezoneMinutes: number;
  committedUnixSeconds: bigint;
  committerTimezoneMinutes: number;
  authorName: Uint8Array;
  authorEmail: Uint8Array;
  committerName: Uint8Array;
  committerEmail: Uint8Array;
  message: Uint8Array;
}

export interface YasGitTreeEntryRecord {
  kind: "tree-entry";
  entryKind: number;
  mode: number;
  name: Uint8Array;
  object: YasGitObjectId;
}

export interface YasGitLogPathRecord {
  kind: "log-path";
  entryKind: number;
  mode: number;
  object?: YasGitObjectId;
  path: YasFsPath;
}

export type YasGitContentDelivery =
  | { kind: "inline"; bytes: Uint8Array }
  | { kind: "transfer"; descriptor: YasTransferDescriptor };

export interface YasGitContentRecord {
  kind: "blob" | "patch";
  object: YasGitObjectId;
  byteLength: bigint;
  offset: bigint;
  nextOffset: bigint;
  delivery: YasGitContentDelivery;
}

export interface YasGitDiffRecord {
  kind: "diff";
  status: number;
  similarityPercent: number;
  flags: number;
  oldPath?: YasFsPath;
  newPath?: YasFsPath;
  oldMode: number;
  newMode: number;
  oldObject?: YasGitObjectId;
  newObject?: YasGitObjectId;
}

export interface YasGitPatchFileRecord {
  kind: "patch-file";
  status: number;
  similarityPercent: number;
  flags: number;
  oldPath?: YasFsPath;
  newPath?: YasFsPath;
}

export interface YasGitPatchSpan {
  start: number;
  length: number;
}

export interface YasGitPatchRowRecord {
  kind: "patch-row";
  oldLine: number;
  newLine: number;
  oldText: Uint8Array;
  newText: Uint8Array;
  oldSpans: readonly YasGitPatchSpan[];
  newSpans: readonly YasGitPatchSpan[];
}

export interface YasGitPatchGapRecord {
  kind: "patch-gap";
  oldLine: number;
  newLine: number;
}

export interface YasGitPatchBaseRecord {
  kind: "patch-base";
  object: YasGitObjectId;
}

export interface YasGitIndexEntryRecord {
  kind: "index-entry";
  stage: number;
  status: number;
  flags: number;
  path: YasFsPath;
  mode: number;
  size: bigint;
  modifiedUnixNs: bigint;
  object: YasGitObjectId;
}

export interface YasGitDiscoveryRecord {
  kind: "discovery";
  flags: number;
  objectAlgorithm: number;
  worktreePath: Uint8Array;
  gitDir: Uint8Array;
}

export interface YasGitBlameRecord {
  kind: "blame";
  flags: number;
  startLine: number;
  endLine: number;
  originalStartLine: number;
  commit: YasGitObjectId;
  originalPath?: YasFsPath;
  author: Uint8Array;
  summary: Uint8Array;
}

export interface YasGitReflogRecord {
  kind: "reflog";
  flags: number;
  index: bigint;
  oldObject: YasGitObjectId;
  newObject: YasGitObjectId;
  committer: Uint8Array;
  committedUnixSeconds: bigint;
  timezoneMinutes: number;
  message: Uint8Array;
}

export interface YasGitWorktreeRecord {
  kind: "worktree";
  flags: number;
  path: Uint8Array;
  head?: YasGitObjectId;
  branch: Uint8Array;
  lockReason: string;
}

export interface YasGitObjectRecord {
  kind: "object";
  role: number;
  object: YasGitObjectId;
}

export type YasGitQueryRecord =
  | YasGitObjectRecord
  | YasGitCommitRecord
  | YasGitLogPathRecord
  | YasGitTreeEntryRecord
  | YasGitContentRecord
  | YasGitDiffRecord
  | YasGitPatchFileRecord
  | YasGitPatchRowRecord
  | YasGitPatchGapRecord
  | YasGitPatchBaseRecord
  | YasGitIndexEntryRecord
  | YasGitDiscoveryRecord
  | YasGitBlameRecord
  | YasGitReflogRecord
  | YasGitWorktreeRecord;

export type YasGitPageDelivery =
  | { kind: "inline"; records: readonly YasGitQueryRecord[] }
  | { kind: "transfer"; descriptor: YasTransferDescriptor };

export interface YasGitQueryPageWire {
  nextCursor: YasGitQueryCursor;
  totalHint: bigint;
  flags: number;
  delivery: YasGitPageDelivery;
  extensions: readonly YasExtension[];
}

export interface YasGitQueryPage {
  nextCursor: YasGitQueryCursor;
  totalHint: bigint;
  flags: number;
  records(): Promise<readonly YasGitQueryRecord[]>;
}

export interface YasGitQueryState {
  querySubscriptionId: number;
  event: {
    subscriptionId: number;
    batch: YasStateBatch;
  };
}

export interface YasGitWatchedQueryValue {
  status: number;
  detail: string;
  page?: YasGitQueryPageWire;
}

export interface YasGitWatchedQueryUpdate {
  status: number;
  detail: string;
  page?: YasGitQueryPage;
}

export interface YasGitFetch {
  repositoryHandle: bigint;
  operationId: Uint8Array;
  flags: number;
  timeoutMs: number;
  remote: Uint8Array;
  refspecs: readonly Uint8Array[];
  extensions?: readonly YasExtension[];
}

export interface YasGitFetchResult {
  repositoryRevision: bigint;
  refs: readonly YasGitFetchRefResult[];
  extensions: readonly YasExtension[];
}

export interface YasGitFetchRefResult {
  flags: number;
  status: number;
  old?: YasGitObjectId;
  new?: YasGitObjectId;
  name: Uint8Array;
  detail: string;
}

export interface YasGitProgress {
  operationId: Uint8Array;
  phase: number;
  flags: number;
  current: bigint;
  total: bigint;
  message: string;
}

export interface YasGitClosed {
  repositoryHandle: bigint;
  repositoryRevision: bigint;
  reason: number;
  detail: string;
}

export interface YasGitHeadEntityBody {
  kind: "head";
  flags: number;
  object?: YasGitObjectId;
  symbolicTarget: Uint8Array;
}

export interface YasGitRefEntityBody {
  kind: "ref";
  flags: number;
  object: YasGitObjectId;
  peeled?: YasGitObjectId;
  symbolicTarget: Uint8Array;
}

export interface YasGitRemoteEntityBody {
  kind: "remote";
  flags: number;
  fetchUrl: Uint8Array;
  pushUrl: Uint8Array;
}

export interface YasGitOperationEntityBody {
  kind: "operation";
  operationKind: number;
  flags: number;
  head?: YasGitObjectId;
  detail: string;
}

export interface YasGitStatusEntityBody {
  kind: "status";
  indexStatus: number;
  worktreeStatus: number;
  flags: number;
  content?: YasGitObjectId;
  oldPath?: YasFsPath;
}

export interface YasGitUpstreamEntityBody {
  kind: "upstream";
  flags: number;
  ahead: number;
  behind: number;
  upstream: Uint8Array;
}

export interface YasGitStashEntityBody {
  kind: "stash";
  object: YasGitObjectId;
  createdUnixSeconds: bigint;
  timezoneMinutes: number;
  message: Uint8Array;
}

export interface YasGitWorktreeGenerationEntityBody {
  kind: "worktree-generation";
  count: number;
  digest: bigint;
}

export type YasGitEntityBody =
  | YasGitHeadEntityBody
  | YasGitRefEntityBody
  | YasGitRemoteEntityBody
  | YasGitOperationEntityBody
  | YasGitStatusEntityBody
  | YasGitUpstreamEntityBody
  | YasGitStashEntityBody
  | YasGitWorktreeGenerationEntityBody;

export interface YasGitEntityRecord {
  entityKind: number;
  key: Uint8Array;
  revision: bigint;
  body: YasGitEntityBody;
  extensions: readonly YasExtension[];
}

export interface YasGitEntityPatch {
  entityKind: number;
  key: Uint8Array;
  observedRevision: bigint;
  fields: number;
  replacement: YasGitEntityRecord;
  extensions: readonly YasExtension[];
}

export interface YasGitRemovedEntity {
  entityKind: number;
  key: Uint8Array;
  revision: bigint;
}

export interface YasGitSnapshot {
  revision: bigint;
  entities: readonly YasGitEntityRecord[];
}

export function encodeGitObjectId(value: YasGitObjectId): Uint8Array {
  validateObjectId(value);
  return new YasWriter()
    .u8(value.algorithm)
    .u8(value.bytes.length)
    .u16(0)
    .bytes(value.bytes)
    .finish();
}

export function decodeGitObjectId(bytes: Uint8Array): YasGitObjectId {
  const cursor = new YasCursor(bytes);
  const value = decodeObjectId(cursor);
  cursor.end("Git object ID");
  return value;
}

export function encodeGitRepositorySource(
  value: YasGitRepositorySource,
): Uint8Array {
  const writer = new YasWriter();
  if (value.kind === "platform-path") {
    if (
      value.path.length === 0 ||
      value.path.length > g.YAS_GIT_MAX_PATH_BYTES ||
      value.path.includes(0)
    )
      throw new YasProtocolError("invalid Git platform path");
    writer
      .u8(g.YAS_GIT_SOURCE_PLATFORM_PATH)
      .bytes(new Uint8Array(3))
      .bytesU32(value.path);
  } else if (value.kind === "fs") {
    requireHandle(value.rootHandle, "Git FS root handle");
    writer
      .u8(g.YAS_GIT_SOURCE_FS)
      .bytes(new Uint8Array(3))
      .u64(value.rootHandle)
      .bytesU32(encodeFsPath(value.path));
  } else if (value.kind === "submodule") {
    requireHandle(value.parentRepository, "parent Git repository handle");
    requireNonRootPath(value.path);
    writer
      .u8(g.YAS_GIT_SOURCE_SUBMODULE)
      .bytes(new Uint8Array(3))
      .u64(value.parentRepository)
      .bytesU32(encodeFsPath(value.path));
  } else {
    requireHandle(value.terminalHandle, "Git Terminal handle");
    writer
      .u8(g.YAS_GIT_SOURCE_TERMINAL_CWD)
      .bytes(new Uint8Array(3))
      .u64(value.terminalHandle)
      .bytesU32(encodeFsPath(value.suffix));
  }
  return writer.finish();
}

export function decodeGitRepositorySource(
  bytes: Uint8Array,
): YasGitRepositorySource {
  const cursor = new YasCursor(bytes);
  const kind = cursor.u8("Git repository source kind");
  requireZero(cursor.take(3, "Git source reserved"), "Git source");
  let value: YasGitRepositorySource;
  if (kind === g.YAS_GIT_SOURCE_PLATFORM_PATH)
    value = {
      kind: "platform-path",
      path: new Uint8Array(cursor.bytesU32("Git platform path")),
    };
  else if (kind === g.YAS_GIT_SOURCE_FS)
    value = {
      kind: "fs",
      rootHandle: cursor.u64("Git FS root handle"),
      path: decodeFsPath(cursor.bytesU32("Git FS path")),
    };
  else if (kind === g.YAS_GIT_SOURCE_SUBMODULE)
    value = {
      kind: "submodule",
      parentRepository: cursor.u64("parent Git repository handle"),
      path: decodeFsPath(cursor.bytesU32("Git submodule path")),
    };
  else if (kind === g.YAS_GIT_SOURCE_TERMINAL_CWD)
    value = {
      kind: "terminal-cwd",
      terminalHandle: cursor.u64("Git Terminal handle"),
      suffix: decodeFsPath(cursor.bytesU32("Git Terminal CWD suffix")),
    };
  else throw new YasProtocolError("unknown Git repository source kind");
  cursor.end("Git repository source");
  encodeGitRepositorySource(value);
  return value;
}

export function encodeGitOpen(value: YasGitOpen): Uint8Array {
  rejectRequired(value.extensions, "Git OPEN");
  return new YasWriter()
    .bytesU32(encodeGitRepositorySource(value.source))
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeGitOpen(bytes: Uint8Array): YasGitOpen {
  const cursor = new YasCursor(bytes);
  const value = {
    source: decodeGitRepositorySource(cursor.bytesU32("Git source")),
    extensions: decodeExtensions(cursor, new Set(), "Git OPEN extensions"),
  };
  cursor.end("Git OPEN");
  encodeGitOpen(value);
  return value;
}

export function encodeGitOpenResult(value: YasGitOpenResult): Uint8Array {
  validateOpenResult(value);
  return new YasWriter()
    .u64(value.repositoryHandle)
    .u64(value.repositoryRevision)
    .u8(value.objectAlgorithm)
    .u8(0)
    .u16(value.repositoryFlags)
    .bytesU32(value.canonicalWorktreePath)
    .bytesU32(value.canonicalGitDir)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeGitOpenResult(bytes: Uint8Array): YasGitOpenResult {
  const cursor = new YasCursor(bytes);
  const repositoryHandle = cursor.u64("Git repository handle");
  const repositoryRevision = cursor.u64("Git repository revision");
  const objectAlgorithm = cursor.u8("Git object algorithm");
  if (cursor.u8("Git OPEN Result reserved") !== 0)
    throw new YasProtocolError("Git OPEN Result reserved field is nonzero");
  const value = {
    repositoryHandle,
    repositoryRevision,
    objectAlgorithm,
    repositoryFlags: cursor.u16("Git repository flags"),
    canonicalWorktreePath: new Uint8Array(
      cursor.bytesU32("Git canonical worktree path"),
    ),
    canonicalGitDir: new Uint8Array(cursor.bytesU32("Git canonical Git dir")),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Git OPEN Result extensions",
    ),
  };
  cursor.end("Git OPEN Result");
  validateOpenResult(value);
  return value;
}

export function encodeGitWatchOptions(
  value: YasGitWatchOptions,
): YasExtension[] {
  const refsSettleMs = value.refsSettleMs ?? 0;
  const statusSettleMs = value.statusSettleMs ?? 0;
  const prefixes = value.refPrefixes ?? [];
  const extensions: YasExtension[] = [];
  if (refsSettleMs !== 0)
    extensions.push({
      tag: g.YAS_GIT_WATCH_REFS_SETTLE_MS_EXTENSION,
      required: false,
      value: new YasWriter().u16(refsSettleMs).finish(),
    });
  if (statusSettleMs !== 0)
    extensions.push({
      tag: g.YAS_GIT_WATCH_STATUS_SETTLE_MS_EXTENSION,
      required: false,
      value: new YasWriter().u16(statusSettleMs).finish(),
    });
  if (prefixes.length !== 0) {
    if (prefixes.length > g.YAS_GIT_MAX_REF_PREFIXES)
      throw new YasProtocolError("too many Git ref prefixes");
    const writer = new YasWriter().u16(prefixes.length);
    let previous: Uint8Array | undefined;
    for (const prefix of prefixes) {
      validateSpec(prefix);
      if (previous && compareBytes(previous, prefix) >= 0)
        throw new YasProtocolError("Git ref prefixes are not strictly sorted");
      writer.bytesU16(prefix);
      previous = prefix;
    }
    extensions.push({
      tag: g.YAS_GIT_WATCH_REF_PREFIXES_EXTENSION,
      required: false,
      value: writer.finish(),
    });
  }
  return extensions;
}

export function decodeGitWatchOptions(
  extensions: readonly YasExtension[],
): YasGitWatchOptions {
  rejectUnknownRequired(
    extensions,
    new Set([
      g.YAS_GIT_WATCH_REFS_SETTLE_MS_EXTENSION,
      g.YAS_GIT_WATCH_STATUS_SETTLE_MS_EXTENSION,
      g.YAS_GIT_WATCH_REF_PREFIXES_EXTENSION,
    ]),
    "Git WATCH",
  );
  const settle = (tag: number): number => {
    const extension = extensions.find((entry) => entry.tag === tag);
    if (!extension) return 0;
    const cursor = new YasCursor(extension.value);
    const result = cursor.u16("Git settle time");
    cursor.end("Git settle extension");
    return result;
  };
  const refPrefixes: Uint8Array[] = [];
  const prefixExtension = extensions.find(
    (entry) => entry.tag === g.YAS_GIT_WATCH_REF_PREFIXES_EXTENSION,
  );
  if (prefixExtension) {
    const cursor = new YasCursor(prefixExtension.value);
    const count = cursor.u16("Git ref prefix count");
    if (count > g.YAS_GIT_MAX_REF_PREFIXES || count > cursor.remaining / 2)
      throw new YasProtocolError("invalid Git ref prefix count");
    for (let index = 0; index < count; index++)
      refPrefixes.push(new Uint8Array(cursor.bytesU16("Git ref prefix")));
    cursor.end("Git ref prefixes");
  }
  const result = {
    refsSettleMs: settle(g.YAS_GIT_WATCH_REFS_SETTLE_MS_EXTENSION),
    statusSettleMs: settle(g.YAS_GIT_WATCH_STATUS_SETTLE_MS_EXTENSION),
    refPrefixes,
  };
  encodeGitWatchOptions(result);
  return result;
}

export function encodeGitClose(
  repositoryHandle: bigint,
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  requireHandle(repositoryHandle, "Git repository handle");
  rejectRequired(extensions, "Git CLOSE");
  return new YasWriter()
    .u64(repositoryHandle)
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function decodeGitClose(bytes: Uint8Array): {
  repositoryHandle: bigint;
  extensions: YasExtension[];
} {
  const cursor = new YasCursor(bytes);
  const value = {
    repositoryHandle: cursor.u64("Git repository handle"),
    extensions: decodeExtensions(cursor, new Set(), "Git CLOSE extensions"),
  };
  cursor.end("Git CLOSE");
  encodeGitClose(value.repositoryHandle, value.extensions);
  return value;
}

export function encodeGitWatch(
  repositoryHandle: bigint,
  datasets: number,
  encodedStateWatch: Uint8Array,
): Uint8Array {
  requireHandle(repositoryHandle, "Git repository handle");
  if (datasets === 0 || datasets & ~g.YAS_GIT_WATCH_DATASETS)
    throw new YasProtocolError("invalid Git WATCH datasets");
  return new YasWriter()
    .u64(repositoryHandle)
    .u16(datasets)
    .u16(0)
    .bytesU32(encodedStateWatch)
    .finish();
}

export function decodeGitWatch(bytes: Uint8Array): {
  repositoryHandle: bigint;
  datasets: number;
  encodedStateWatch: Uint8Array;
} {
  const cursor = new YasCursor(bytes);
  const repositoryHandle = cursor.u64("Git repository handle");
  const datasets = cursor.u16("Git WATCH datasets");
  if (cursor.u16("Git WATCH reserved") !== 0)
    throw new YasProtocolError("Git WATCH reserved field is nonzero");
  const encodedStateWatch = new Uint8Array(cursor.bytesU32("Git State WATCH"));
  cursor.end("Git WATCH");
  encodeGitWatch(repositoryHandle, datasets, encodedStateWatch);
  return { repositoryHandle, datasets, encodedStateWatch };
}

export function encodeGitUnwatch(subscriptionId: number): Uint8Array {
  if (subscriptionId === 0)
    throw new YasProtocolError("zero Git subscription ID");
  return new YasWriter().u32(subscriptionId).finish();
}

export function decodeGitUnwatch(bytes: Uint8Array): number {
  const cursor = new YasCursor(bytes);
  const value = cursor.u32("Git subscription ID");
  cursor.end("Git UNWATCH");
  encodeGitUnwatch(value);
  return value;
}

export function encodeGitQueryEndpoint(value: YasGitQueryEndpoint): Uint8Array {
  let kind: number;
  let object: YasGitObjectId | undefined;
  if (value.kind === "empty") kind = g.YAS_GIT_ENDPOINT_EMPTY;
  else if (value.kind === "commit") {
    kind = g.YAS_GIT_ENDPOINT_COMMIT;
    object = value.object;
  } else if (value.kind === "tree") {
    kind = g.YAS_GIT_ENDPOINT_TREE;
    object = value.object;
  } else if (value.kind === "index") kind = g.YAS_GIT_ENDPOINT_INDEX;
  else if (value.kind === "worktree") kind = g.YAS_GIT_ENDPOINT_WORKTREE;
  else {
    kind = g.YAS_GIT_ENDPOINT_MERGE_BASE;
    object = value.object;
  }
  return new YasWriter()
    .u8(kind)
    .bytes(new Uint8Array(3))
    .u8(object ? 1 : 0)
    .bytes(new Uint8Array(3))
    .bytes(object ? encodeGitObjectId(object) : new Uint8Array())
    .finish();
}

function decodeGitQueryEndpointFrom(cursor: YasCursor): YasGitQueryEndpoint {
  const kind = cursor.u8("Git endpoint kind");
  requireZero(cursor.take(3, "Git endpoint reserved"), "Git endpoint");
  const present = cursor.u8("Git endpoint object presence");
  requireZero(cursor.take(3, "Git endpoint reserved"), "Git endpoint");
  if (present > 1) throw new YasProtocolError("invalid Git endpoint presence");
  const object = present ? decodeObjectId(cursor) : undefined;
  if (kind === g.YAS_GIT_ENDPOINT_EMPTY && !object) return { kind: "empty" };
  if (kind === g.YAS_GIT_ENDPOINT_COMMIT && object)
    return { kind: "commit", object };
  if (kind === g.YAS_GIT_ENDPOINT_TREE && object)
    return { kind: "tree", object };
  if (kind === g.YAS_GIT_ENDPOINT_INDEX && !object) return { kind: "index" };
  if (kind === g.YAS_GIT_ENDPOINT_WORKTREE && !object)
    return { kind: "worktree" };
  if (kind === g.YAS_GIT_ENDPOINT_MERGE_BASE && object)
    return { kind: "merge-base", object };
  throw new YasProtocolError("invalid Git endpoint kind or object presence");
}

export function decodeGitQueryEndpoint(bytes: Uint8Array): YasGitQueryEndpoint {
  const cursor = new YasCursor(bytes);
  const value = decodeGitQueryEndpointFrom(cursor);
  cursor.end("Git endpoint");
  return value;
}

export function encodeGitQueryCursor(value: YasGitQueryCursor): Uint8Array {
  if (value.kind === "start") return new Uint8Array();
  const writer = new YasWriter();
  if (value.kind === "log-frontier") {
    if (
      value.objects.length === 0 ||
      value.objects.length > g.YAS_GIT_MAX_QUERY_ENDPOINTS
    )
      throw new YasProtocolError("invalid Git log frontier count");
    const seen = new Set<string>();
    writer
      .u8(g.YAS_GIT_CURSOR_LOG_FRONTIER)
      .bytes(new Uint8Array(3))
      .u16(value.objects.length)
      .u16(0);
    for (const object of value.objects) {
      const key = hex(encodeGitObjectId(object));
      if (seen.has(key))
        throw new YasProtocolError("duplicate Git log frontier object");
      seen.add(key);
      writer.bytes(encodeGitObjectId(object));
    }
  } else if (value.kind === "path") {
    requireNonRootPath(value.path);
    writer
      .u8(g.YAS_GIT_CURSOR_PATH)
      .bytes(new Uint8Array(3))
      .bytesU32(encodeFsPath(value.path));
  } else if (value.kind === "platform-path") {
    validatePlatformPath(value.path, "Git platform cursor path");
    writer
      .u8(g.YAS_GIT_CURSOR_PLATFORM_PATH)
      .bytes(new Uint8Array(3))
      .bytesU32(value.path);
  } else if (value.kind === "patch") {
    requireNonRootPath(value.path);
    writer
      .u8(g.YAS_GIT_CURSOR_PATCH)
      .bytes(new Uint8Array(3))
      .bytesU32(encodeFsPath(value.path))
      .u64(value.position);
  } else {
    writer
      .u8(g.YAS_GIT_CURSOR_POSITION)
      .bytes(new Uint8Array(3))
      .u64(value.position);
  }
  const result = writer.finish();
  if (result.length > g.YAS_GIT_MAX_CURSOR_BYTES)
    throw new YasProtocolError("Git query cursor exceeds limit");
  return result;
}

export function decodeGitQueryCursor(bytes: Uint8Array): YasGitQueryCursor {
  if (bytes.length === 0) return { kind: "start" };
  const cursor = new YasCursor(bytes);
  const kind = cursor.u8("Git cursor kind");
  requireZero(cursor.take(3, "Git cursor reserved"), "Git cursor");
  let value: YasGitQueryCursor;
  if (kind === g.YAS_GIT_CURSOR_LOG_FRONTIER) {
    const count = cursor.u16("Git frontier count");
    if (
      cursor.u16("Git frontier reserved") !== 0 ||
      count === 0 ||
      count > g.YAS_GIT_MAX_QUERY_ENDPOINTS ||
      count > cursor.remaining / 24
    )
      throw new YasProtocolError("invalid Git log frontier count");
    const objects: YasGitObjectId[] = [];
    for (let index = 0; index < count; index++)
      objects.push(decodeObjectId(cursor));
    value = { kind: "log-frontier", objects };
  } else if (kind === g.YAS_GIT_CURSOR_PATH)
    value = {
      kind: "path",
      path: decodeFsPath(cursor.bytesU32("Git path cursor")),
    };
  else if (kind === g.YAS_GIT_CURSOR_PLATFORM_PATH)
    value = {
      kind: "platform-path",
      path: new Uint8Array(cursor.bytesU32("Git platform cursor path")),
    };
  else if (kind === g.YAS_GIT_CURSOR_PATCH)
    value = {
      kind: "patch",
      path: decodeFsPath(cursor.bytesU32("Git patch cursor path")),
      position: cursor.u64("Git patch cursor position"),
    };
  else if (kind === g.YAS_GIT_CURSOR_POSITION)
    value = { kind: "position", position: cursor.u64("Git cursor position") };
  else throw new YasProtocolError("unknown Git query cursor kind");
  cursor.end("Git query cursor");
  const encoded = encodeGitQueryCursor(value);
  if (!equal(encoded, bytes))
    throw new YasProtocolError("noncanonical Git cursor");
  return value;
}

export function encodeGitQueryBody(value: YasGitQueryBody): Uint8Array {
  let kind: number;
  const body = new YasWriter();
  if (value.kind === "resolve") {
    kind = g.YAS_GIT_QUERY_RESOLVE;
    validateSpec(value.spec);
    body.bytesU16(value.spec);
  } else if (value.kind === "merge-base") {
    kind = g.YAS_GIT_QUERY_MERGE_BASE;
    if (
      value.objects.length < 2 ||
      value.objects.length > g.YAS_GIT_MAX_QUERY_ENDPOINTS
    )
      throw new YasProtocolError("invalid Git MERGE_BASE object count");
    body.u16(value.objects.length).u16(0);
    for (const object of value.objects) body.bytes(encodeGitObjectId(object));
  } else if (value.kind === "log") {
    kind = g.YAS_GIT_QUERY_LOG;
    if (value.flags & ~g.YAS_GIT_LOG_FLAGS)
      throw new YasProtocolError("invalid Git LOG flags");
    if (
      value.tips.length > g.YAS_GIT_MAX_QUERY_ENDPOINTS ||
      value.hides.length > g.YAS_GIT_MAX_QUERY_ENDPOINTS ||
      (value.spec.length !== 0 &&
        (value.tips.length !== 0 || value.hides.length !== 0)) ||
      (value.spec.length === 0 &&
        value.tips.length === 0 &&
        value.hides.length !== 0)
    )
      throw new YasProtocolError("invalid Git LOG seed endpoints");
    if (value.spec.length !== 0) validateSpec(value.spec);
    body
      .u16(value.flags)
      .u16(0)
      .bytesU16(value.spec)
      .u16(value.tips.length)
      .u16(value.hides.length);
    for (const object of [...value.tips, ...value.hides])
      body.bytes(encodeGitObjectId(object));
    body.bytesU32(value.path ? encodeFsPath(value.path) : new Uint8Array());
  } else if (value.kind === "tree") {
    kind = g.YAS_GIT_QUERY_TREE;
    body
      .bytes(encodeGitObjectId(value.tree))
      .bytesU32(encodeFsPath(value.path));
  } else if (value.kind === "blob") {
    kind = g.YAS_GIT_QUERY_BLOB;
    if (value.flags & ~g.YAS_GIT_BLOB_FLAGS)
      throw new YasProtocolError("invalid Git BLOB flags");
    body
      .u16(value.flags)
      .u16(0)
      .bytes(encodeGitObjectId(value.object))
      .bytesU32(value.path ? encodeFsPath(value.path) : new Uint8Array())
      .u64(value.offset)
      .u32(value.maxBytes);
  } else if (value.kind === "diff") {
    kind = g.YAS_GIT_QUERY_DIFF;
    if (
      value.flags & ~g.YAS_GIT_DIFF_FLAGS ||
      value.renameThreshold > g.YAS_GIT_RENAME_THRESHOLD_MAX ||
      value.right.kind === "merge-base" ||
      (value.left.kind === "empty" && value.right.kind === "empty")
    )
      throw new YasProtocolError("invalid Git DIFF flags");
    body
      .u16(value.flags)
      .u8(value.renameThreshold)
      .u8(0)
      .bytes(encodeGitQueryEndpoint(value.left))
      .bytes(encodeGitQueryEndpoint(value.right))
      .bytesU32(value.path ? encodeFsPath(value.path) : new Uint8Array());
  } else if (value.kind === "patch") {
    kind = g.YAS_GIT_QUERY_PATCH;
    if (
      value.flags & ~g.YAS_GIT_PATCH_FLAGS ||
      value.renameThreshold > g.YAS_GIT_RENAME_THRESHOLD_MAX ||
      value.right.kind === "merge-base" ||
      (value.left.kind === "empty" && value.right.kind === "empty")
    )
      throw new YasProtocolError("invalid Git PATCH flags");
    body
      .u16(value.flags)
      .u8(value.contextLines)
      .u8(value.renameThreshold)
      .u32(value.maxBytes)
      .bytes(encodeGitQueryEndpoint(value.left))
      .bytes(encodeGitQueryEndpoint(value.right))
      .bytesU32(value.path ? encodeFsPath(value.path) : new Uint8Array());
  } else if (value.kind === "index") {
    kind = g.YAS_GIT_QUERY_INDEX;
    if (value.flags & ~g.YAS_GIT_INDEX_FLAGS)
      throw new YasProtocolError("invalid Git INDEX flags");
    body
      .u16(value.flags)
      .u16(0)
      .bytesU32(value.path ? encodeFsPath(value.path) : new Uint8Array());
  } else if (value.kind === "discover") {
    kind = g.YAS_GIT_QUERY_DISCOVER;
    if (value.flags & ~g.YAS_GIT_DISCOVER_QUERY_FLAGS)
      throw new YasProtocolError("invalid Git DISCOVER flags");
    body
      .u16(value.flags)
      .u16(value.maxDepth)
      .bytesU32(encodeGitRepositorySource(value.source));
  } else if (value.kind === "blame") {
    kind = g.YAS_GIT_QUERY_BLAME;
    if (value.startLine === 0 || value.flags & ~g.YAS_GIT_BLAME_FLAGS)
      throw new YasProtocolError("invalid Git BLAME line range");
    body
      .u16(value.flags)
      .u16(0)
      .bytes(encodeGitObjectId(value.object))
      .bytesU32(encodeFsPath(value.path))
      .u32(value.startLine)
      .u32(value.lineCount);
  } else if (value.kind === "reflog") {
    kind = g.YAS_GIT_QUERY_REFLOG;
    if (value.flags & ~g.YAS_GIT_REFLOG_FLAGS)
      throw new YasProtocolError("invalid Git REFLOG flags");
    if (value.name.length !== 0) validateSpec(value.name);
    body.u16(value.flags).u16(0).bytesU16(value.name);
  } else kind = g.YAS_GIT_QUERY_WORKTREES;
  return new YasWriter().u16(kind).u16(0).bytes(body.finish()).finish();
}

export function decodeGitQueryBody(bytes: Uint8Array): YasGitQueryBody {
  const cursor = new YasCursor(bytes);
  const kind = cursor.u16("Git query kind");
  if (cursor.u16("Git query flags") !== 0)
    throw new YasProtocolError("Git query flags are nonzero");
  let value: YasGitQueryBody;
  if (kind === g.YAS_GIT_QUERY_RESOLVE)
    value = {
      kind: "resolve",
      spec: new Uint8Array(cursor.bytesU16("Git spec")),
    };
  else if (kind === g.YAS_GIT_QUERY_MERGE_BASE) {
    const count = cursor.u16("Git MERGE_BASE object count");
    if (
      cursor.u16("Git MERGE_BASE reserved") !== 0 ||
      count < 2 ||
      count > g.YAS_GIT_MAX_QUERY_ENDPOINTS ||
      count > cursor.remaining / 24
    )
      throw new YasProtocolError("invalid Git MERGE_BASE object count");
    const objects: YasGitObjectId[] = [];
    for (let index = 0; index < count; index++)
      objects.push(decodeObjectId(cursor));
    value = { kind: "merge-base", objects };
  } else if (kind === g.YAS_GIT_QUERY_LOG) {
    const flags = cursor.u16("Git LOG flags");
    if (cursor.u16("Git LOG reserved") !== 0)
      throw new YasProtocolError("Git LOG reserved field is nonzero");
    const spec = new Uint8Array(cursor.bytesU16("Git LOG spec"));
    const tipCount = cursor.u16("Git LOG tip count");
    const hideCount = cursor.u16("Git LOG hide count");
    if (
      tipCount > g.YAS_GIT_MAX_QUERY_ENDPOINTS ||
      hideCount > g.YAS_GIT_MAX_QUERY_ENDPOINTS ||
      tipCount + hideCount > cursor.remaining / 24
    )
      throw new YasProtocolError("invalid Git LOG endpoint count");
    const tips: YasGitObjectId[] = [];
    const hides: YasGitObjectId[] = [];
    for (let index = 0; index < tipCount; index++)
      tips.push(decodeObjectId(cursor));
    for (let index = 0; index < hideCount; index++)
      hides.push(decodeObjectId(cursor));
    const path = cursor.bytesU32("Git LOG path");
    value = {
      kind: "log",
      spec,
      tips,
      hides,
      path: path.length ? decodeFsPath(path) : undefined,
      flags,
    };
  } else if (kind === g.YAS_GIT_QUERY_TREE)
    value = {
      kind: "tree",
      tree: decodeObjectId(cursor),
      path: decodeFsPath(cursor.bytesU32("Git tree path")),
    };
  else if (kind === g.YAS_GIT_QUERY_BLOB) {
    const flags = cursor.u16("Git BLOB flags");
    if (cursor.u16("Git BLOB reserved") !== 0)
      throw new YasProtocolError("Git BLOB reserved field is nonzero");
    const object = decodeObjectId(cursor);
    const path = cursor.bytesU32("Git BLOB path");
    value = {
      kind: "blob",
      flags,
      object,
      path: path.length ? decodeFsPath(path) : undefined,
      offset: cursor.u64("Git BLOB offset"),
      maxBytes: cursor.u32("Git BLOB maximum bytes"),
    };
  } else if (kind === g.YAS_GIT_QUERY_DIFF) {
    const flags = cursor.u16("Git DIFF flags");
    const renameThreshold = cursor.u8("Git DIFF rename threshold");
    if (cursor.u8("Git DIFF reserved") !== 0)
      throw new YasProtocolError("Git DIFF reserved field is nonzero");
    const left = decodeGitQueryEndpointFrom(cursor);
    const right = decodeGitQueryEndpointFrom(cursor);
    const path = cursor.bytesU32("Git DIFF path");
    value = {
      kind: "diff",
      left,
      right,
      path: path.length ? decodeFsPath(path) : undefined,
      renameThreshold,
      flags,
    };
  } else if (kind === g.YAS_GIT_QUERY_PATCH) {
    const flags = cursor.u16("Git PATCH flags");
    const contextLines = cursor.u8("Git PATCH context lines");
    const renameThreshold = cursor.u8("Git PATCH rename threshold");
    const maxBytes = cursor.u32("Git PATCH maximum bytes");
    const left = decodeGitQueryEndpointFrom(cursor);
    const right = decodeGitQueryEndpointFrom(cursor);
    const path = cursor.bytesU32("Git PATCH path");
    value = {
      kind: "patch",
      left,
      right,
      path: path.length ? decodeFsPath(path) : undefined,
      contextLines,
      renameThreshold,
      maxBytes,
      flags,
    };
  } else if (kind === g.YAS_GIT_QUERY_INDEX) {
    const flags = cursor.u16("Git INDEX flags");
    if (cursor.u16("Git INDEX reserved") !== 0)
      throw new YasProtocolError("Git INDEX reserved field is nonzero");
    const path = cursor.bytesU32("Git INDEX path");
    value = {
      kind: "index",
      path: path.length ? decodeFsPath(path) : undefined,
      flags,
    };
  } else if (kind === g.YAS_GIT_QUERY_DISCOVER)
    value = {
      kind: "discover",
      flags: cursor.u16("Git DISCOVER flags"),
      maxDepth: cursor.u16("Git DISCOVER maximum depth"),
      source: decodeGitRepositorySource(
        cursor.bytesU32("Git discovery source"),
      ),
    };
  else if (kind === g.YAS_GIT_QUERY_BLAME) {
    const flags = cursor.u16("Git BLAME flags");
    if (cursor.u16("Git BLAME reserved") !== 0)
      throw new YasProtocolError("Git BLAME reserved field is nonzero");
    value = {
      kind: "blame",
      flags,
      object: decodeObjectId(cursor),
      path: decodeFsPath(cursor.bytesU32("Git blame path")),
      startLine: cursor.u32("Git blame start line"),
      lineCount: cursor.u32("Git blame line count"),
    };
  } else if (kind === g.YAS_GIT_QUERY_REFLOG) {
    const flags = cursor.u16("Git REFLOG flags");
    if (cursor.u16("Git REFLOG reserved") !== 0)
      throw new YasProtocolError("Git REFLOG reserved field is nonzero");
    value = {
      kind: "reflog",
      flags,
      name: new Uint8Array(cursor.bytesU16("Git reflog name")),
    };
  } else if (kind === g.YAS_GIT_QUERY_WORKTREES) value = { kind: "worktrees" };
  else throw new YasProtocolError("unknown Git query kind");
  cursor.end("Git query body");
  encodeGitQueryBody(value);
  return value;
}

export function encodeGitQuery(value: YasGitQuery): Uint8Array {
  const discover = value.body.kind === "discover";
  if (discover !== (value.repositoryHandle === 0n))
    throw new YasProtocolError("invalid Git QUERY repository scope");
  if (!discover) requireHandle(value.repositoryHandle, "Git repository handle");
  const encodedCursor = encodeGitQueryCursor(value.cursor);
  if (value.maxRecords > g.YAS_GIT_MAX_QUERY_RECORDS)
    throw new YasProtocolError("invalid Git query page limits");
  validateQueryCursorForBody(value.cursor, value.body);
  rejectRequired(value.extensions, "Git QUERY");
  return new YasWriter()
    .u64(value.repositoryHandle)
    .u16(value.maxRecords)
    .u16(0)
    .bytesU16(encodedCursor)
    .u64(value.initialReceiveCredit)
    .bytesU32(encodeGitQueryBody(value.body))
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeGitQuery(bytes: Uint8Array): YasGitQuery {
  const cursor = new YasCursor(bytes);
  const repositoryHandle = cursor.u64("Git repository handle");
  const maxRecords = cursor.u16("Git maximum records");
  if (cursor.u16("Git QUERY reserved") !== 0)
    throw new YasProtocolError("Git QUERY reserved field is nonzero");
  const value = {
    repositoryHandle,
    maxRecords,
    cursor: decodeGitQueryCursor(cursor.bytesU16("Git query cursor")),
    initialReceiveCredit: cursor.u64("Git initial receive credit"),
    body: decodeGitQueryBody(cursor.bytesU32("Git query body")),
    extensions: decodeExtensions(cursor, new Set(), "Git QUERY extensions"),
  };
  cursor.end("Git QUERY");
  encodeGitQuery(value);
  return value;
}

export function encodeGitWatchQuery(
  repositoryHandle: bigint,
  maxRecords: number,
  body: YasGitQueryBody,
  encodedStateWatch: Uint8Array,
): Uint8Array {
  requireHandle(repositoryHandle, "Git repository handle");
  if (maxRecords > g.YAS_GIT_MAX_QUERY_RECORDS)
    throw new YasProtocolError("invalid Git watched query page limit");
  return new YasWriter()
    .u64(repositoryHandle)
    .u16(maxRecords)
    .u16(0)
    .bytesU32(encodeGitQueryBody(body))
    .bytesU32(encodedStateWatch)
    .finish();
}

export function decodeGitWatchQuery(bytes: Uint8Array): {
  repositoryHandle: bigint;
  maxRecords: number;
  body: YasGitQueryBody;
  encodedStateWatch: Uint8Array;
} {
  const cursor = new YasCursor(bytes);
  const repositoryHandle = cursor.u64("Git repository handle");
  const maxRecords = cursor.u16("Git watched query page limit");
  if (cursor.u16("Git WATCH_QUERY reserved") !== 0)
    throw new YasProtocolError("Git WATCH_QUERY reserved field is nonzero");
  const value = {
    repositoryHandle,
    maxRecords,
    body: decodeGitQueryBody(cursor.bytesU32("Git query body")),
    encodedStateWatch: new Uint8Array(cursor.bytesU32("Git State WATCH")),
  };
  cursor.end("Git WATCH_QUERY");
  encodeGitWatchQuery(
    value.repositoryHandle,
    value.maxRecords,
    value.body,
    value.encodedStateWatch,
  );
  return value;
}

export const encodeGitUnwatchQuery = encodeGitUnwatch;
export const decodeGitUnwatchQuery = decodeGitUnwatch;

export function encodeGitQueryState(value: YasGitQueryState): Uint8Array {
  if (
    value.querySubscriptionId === 0 ||
    value.querySubscriptionId !== value.event.subscriptionId
  )
    throw new YasProtocolError("Git query subscription ID mismatch");
  const batch = value.event.batch;
  validateGitQueryStateBatch(batch);
  const writer = new YasWriter()
    .u32(value.event.subscriptionId)
    .u8(batch.phase)
    .u8(batch.flags)
    .u16(0)
    .u64(batch.fromRevision)
    .u64(batch.toRevision)
    .u16(batch.records.length);
  for (const record of batch.records) writer.bytes(encodeTypedRecord(record));
  return new YasWriter()
    .u32(value.querySubscriptionId)
    .bytesU32(writer.finish())
    .finish();
}

export function decodeGitQueryState(bytes: Uint8Array): YasGitQueryState {
  const cursor = new YasCursor(bytes);
  const querySubscriptionId = cursor.u32("Git query subscription ID");
  const event = decodeStateEvent(cursor.bytesU32("Git query State Event"));
  cursor.end("Git QUERY_STATE");
  const value = { querySubscriptionId, event };
  if (querySubscriptionId === 0 || querySubscriptionId !== event.subscriptionId)
    throw new YasProtocolError("Git query subscription ID mismatch");
  // Re-encoding also validates typed-record lengths and canonical markers.
  encodeGitQueryState(value);
  return value;
}

export function encodeGitWatchedQueryValue(
  value: YasGitWatchedQueryValue,
): Uint8Array {
  if (
    !Number.isInteger(value.status) ||
    value.status < 0 ||
    value.status > 0xffff
  )
    throw new YasProtocolError("invalid Git watched query status");
  if (value.status === g.YAS_STATUS_OK) {
    if (value.detail.length !== 0 || !value.page)
      throw new YasProtocolError("invalid Git watched query OK value");
    if (value.page.delivery.kind !== "inline")
      throw new YasProtocolError("watched Git query used a Transfer delivery");
  } else if (value.detail.length === 0 || value.page)
    throw new YasProtocolError("invalid Git watched query failure value");
  return new YasWriter()
    .u16(value.status)
    .u16(0)
    .utf8U32(value.detail)
    .bytesU32(value.page ? encodeGitQueryPage(value.page) : new Uint8Array())
    .finish();
}

export function decodeGitWatchedQueryValue(
  bytes: Uint8Array,
): YasGitWatchedQueryValue {
  const cursor = new YasCursor(bytes);
  const status = cursor.u16("Git watched query status");
  if (cursor.u16("Git watched query reserved") !== 0)
    throw new YasProtocolError("Git watched query reserved field is nonzero");
  const detail = cursor.utf8U32("Git watched query detail");
  const encodedPage = cursor.bytesU32("Git watched query page");
  const value: YasGitWatchedQueryValue = {
    status,
    detail,
    page:
      encodedPage.length === 0 ? undefined : decodeGitQueryPage(encodedPage),
  };
  cursor.end("Git watched query value");
  encodeGitWatchedQueryValue(value);
  return value;
}

function validateGitQueryStateBatch(batch: YasStateBatch): void {
  if (batch.flags !== 0)
    throw new YasProtocolError("Git QUERY_STATE flags are nonzero");
  const expectedKind =
    batch.phase === YAS_STATE_SNAPSHOT_RECORDS
      ? YAS_STATE_ADD
      : batch.phase === YAS_STATE_DELTA
        ? YAS_STATE_REPLACE
        : undefined;
  if (expectedKind === undefined) {
    if (batch.records.length !== 0)
      throw new YasProtocolError("Git QUERY_STATE marker contains records");
    return;
  }
  if (
    batch.records.length !== 1 ||
    batch.records[0].kind !== expectedKind ||
    batch.records[0].flags !== 0
  )
    throw new YasProtocolError("invalid Git QUERY_STATE record");
  decodeGitWatchedQueryValue(batch.records[0].body);
}

export function encodeGitCommitRecord(value: YasGitCommitRecord): Uint8Array {
  if (
    value.flags & ~g.YAS_GIT_COMMIT_FLAGS ||
    value.parents.length > g.YAS_GIT_MAX_COMMIT_PARENTS
  )
    throw new YasProtocolError("too many Git commit parents");
  for (const [field, identity] of [
    ["author name", value.authorName],
    ["author email", value.authorEmail],
    ["committer name", value.committerName],
    ["committer email", value.committerEmail],
  ] as const)
    validateOptionalRaw(
      identity,
      g.YAS_GIT_MAX_IDENTITY_BYTES,
      false,
      `Git ${field}`,
    );
  if (value.message.length > g.YAS_GIT_MAX_MESSAGE_BYTES)
    throw new YasProtocolError("Git commit message exceeds limit");
  validateObjectId(value.object);
  validateObjectId(value.tree);
  if (
    value.tree.algorithm !== value.object.algorithm ||
    value.parents.some((parent) => parent.algorithm !== value.object.algorithm)
  )
    throw new YasProtocolError("Git commit object algorithm mismatch");
  const writer = new YasWriter()
    .u16(value.flags)
    .u16(0)
    .bytes(encodeGitObjectId(value.object))
    .bytes(encodeGitObjectId(value.tree))
    .u16(value.parents.length)
    .u16(0);
  for (const parent of value.parents) writer.bytes(encodeGitObjectId(parent));
  return writer
    .i64(value.authoredUnixSeconds)
    .i16(value.authorTimezoneMinutes)
    .i64(value.committedUnixSeconds)
    .i16(value.committerTimezoneMinutes)
    .bytesU16(value.authorName)
    .bytesU16(value.authorEmail)
    .bytesU16(value.committerName)
    .bytesU16(value.committerEmail)
    .bytesU32(value.message)
    .finish();
}

export function decodeGitCommitRecord(bytes: Uint8Array): YasGitCommitRecord {
  const cursor = new YasCursor(bytes);
  const flags = cursor.u16("Git commit flags");
  if (cursor.u16("Git commit reserved") !== 0)
    throw new YasProtocolError("Git commit reserved field is nonzero");
  const object = decodeObjectId(cursor);
  const tree = decodeObjectId(cursor);
  const count = cursor.u16("Git parent count");
  if (
    cursor.u16("Git commit reserved") !== 0 ||
    count > g.YAS_GIT_MAX_COMMIT_PARENTS ||
    count > Math.floor(cursor.remaining / 24)
  )
    throw new YasProtocolError("invalid Git parent count");
  const parents: YasGitObjectId[] = [];
  for (let index = 0; index < count; index++)
    parents.push(decodeObjectId(cursor));
  const value: YasGitCommitRecord = {
    kind: "commit",
    flags,
    object,
    tree,
    parents,
    authoredUnixSeconds: cursor.i64("Git authored time"),
    authorTimezoneMinutes: cursor.i16("Git author timezone"),
    committedUnixSeconds: cursor.i64("Git committed time"),
    committerTimezoneMinutes: cursor.i16("Git committer timezone"),
    authorName: new Uint8Array(cursor.bytesU16("Git author name")),
    authorEmail: new Uint8Array(cursor.bytesU16("Git author email")),
    committerName: new Uint8Array(cursor.bytesU16("Git committer name")),
    committerEmail: new Uint8Array(cursor.bytesU16("Git committer email")),
    message: new Uint8Array(cursor.bytesU32("Git commit message")),
  };
  cursor.end("Git commit");
  encodeGitCommitRecord(value);
  return value;
}

export function encodeGitTreeEntryRecord(
  value: YasGitTreeEntryRecord,
): Uint8Array {
  if (value.entryKind > g.YAS_GIT_TREE_COMMIT)
    throw new YasProtocolError("invalid Git tree entry kind");
  validateRaw(value.name, g.YAS_FS_MAX_COMPONENT_BYTES, true, "Git tree name");
  return new YasWriter()
    .u8(value.entryKind)
    .bytes(new Uint8Array(3))
    .u32(value.mode)
    .bytesU16(value.name)
    .bytes(encodeGitObjectId(value.object))
    .finish();
}

export function decodeGitTreeEntryRecord(
  bytes: Uint8Array,
): YasGitTreeEntryRecord {
  const cursor = new YasCursor(bytes);
  const entryKind = cursor.u8("Git tree entry kind");
  requireZero(cursor.take(3, "Git tree reserved"), "Git tree entry");
  const value: YasGitTreeEntryRecord = {
    kind: "tree-entry",
    entryKind,
    mode: cursor.u32("Git tree entry mode"),
    name: new Uint8Array(cursor.bytesU16("Git tree entry name")),
    object: decodeObjectId(cursor),
  };
  cursor.end("Git tree entry");
  encodeGitTreeEntryRecord(value);
  return value;
}

export function encodeGitLogPathRecord(value: YasGitLogPathRecord): Uint8Array {
  if (value.entryKind > g.YAS_GIT_TREE_COMMIT)
    throw new YasProtocolError("invalid Git LOG path entry kind");
  requireNonRootPath(value.path);
  if (
    !value.object &&
    (value.entryKind !== g.YAS_GIT_TREE_BLOB || value.mode !== 0)
  )
    throw new YasProtocolError("invalid missing Git LOG path object");
  if (value.object) validateObjectId(value.object);
  return new YasWriter()
    .u8(value.entryKind)
    .u8(value.object ? 1 : 0)
    .u16(0)
    .u32(value.mode)
    .bytes(value.object ? encodeGitObjectId(value.object) : new Uint8Array())
    .bytesU32(encodeFsPath(value.path))
    .finish();
}

export function decodeGitLogPathRecord(bytes: Uint8Array): YasGitLogPathRecord {
  const cursor = new YasCursor(bytes);
  const entryKind = cursor.u8("Git LOG path entry kind");
  const present = cursor.u8("Git LOG path object presence");
  if (present > 1 || cursor.u16("Git LOG path reserved") !== 0)
    throw new YasProtocolError(
      "invalid Git LOG path presence or reserved field",
    );
  const value: YasGitLogPathRecord = {
    kind: "log-path",
    entryKind,
    mode: cursor.u32("Git LOG path mode"),
    object: present ? decodeObjectId(cursor) : undefined,
    path: decodeFsPath(cursor.bytesU32("Git LOG path")),
  };
  cursor.end("Git LOG path record");
  encodeGitLogPathRecord(value);
  return value;
}

export function encodeGitContentRecord(value: YasGitContentRecord): Uint8Array {
  const contentKind =
    value.kind === "blob"
      ? g.YAS_GIT_BLOB_CONTENT_KIND
      : g.YAS_GIT_PATCH_CONTENT_KIND;
  const writer = new YasWriter()
    .bytes(encodeGitObjectId(value.object))
    .u64(value.byteLength)
    .u64(value.offset)
    .u64(value.nextOffset)
    .u8(
      value.delivery.kind === "inline"
        ? g.YAS_GIT_CONTENT_INLINE
        : g.YAS_GIT_CONTENT_TRANSFER,
    )
    .bytes(new Uint8Array(3));
  if (value.delivery.kind === "inline") {
    if (
      value.delivery.bytes.length > g.YAS_GIT_MAX_INLINE_BYTES ||
      value.offset > value.nextOffset ||
      value.nextOffset > value.byteLength ||
      BigInt(value.delivery.bytes.length) !== value.nextOffset - value.offset
    )
      throw new YasProtocolError("invalid inline Git content");
    writer.bytesU32(value.delivery.bytes);
  } else {
    if (value.offset > value.nextOffset || value.nextOffset > value.byteLength)
      throw new YasProtocolError("invalid Git content window");
    validateContentDescriptor(value.delivery.descriptor, contentKind);
    writer.bytesU32(encodeTransferDescriptor(value.delivery.descriptor));
  }
  return writer.finish();
}

export function decodeGitContentRecord(
  bytes: Uint8Array,
  kind: "blob" | "patch",
): YasGitContentRecord {
  const cursor = new YasCursor(bytes);
  const object = decodeObjectId(cursor);
  const byteLength = cursor.u64("Git content byte length");
  const offset = cursor.u64("Git content offset");
  const nextOffset = cursor.u64("Git content next offset");
  const deliveryKind = cursor.u8("Git content delivery");
  requireZero(cursor.take(3, "Git content reserved"), "Git content");
  let delivery: YasGitContentDelivery;
  if (deliveryKind === g.YAS_GIT_CONTENT_INLINE)
    delivery = {
      kind: "inline",
      bytes: new Uint8Array(cursor.bytesU32("Git inline content")),
    };
  else if (deliveryKind === g.YAS_GIT_CONTENT_TRANSFER) {
    const descriptorCursor = new YasCursor(
      cursor.bytesU32("Git content Transfer descriptor"),
    );
    const descriptor = decodeTransferDescriptor(descriptorCursor);
    descriptorCursor.end("Git content Transfer descriptor");
    delivery = { kind: "transfer", descriptor };
  } else throw new YasProtocolError("unknown Git content delivery");
  const value: YasGitContentRecord = {
    kind,
    object,
    byteLength,
    offset,
    nextOffset,
    delivery,
  };
  cursor.end("Git content");
  encodeGitContentRecord(value);
  return value;
}

export function encodeGitDiffRecord(value: YasGitDiffRecord): Uint8Array {
  if (
    value.status > g.YAS_GIT_DIFF_COPIED ||
    value.similarityPercent > 100 ||
    value.flags & ~g.YAS_GIT_DIFF_RECORD_FLAGS ||
    (!value.oldPath && !value.newPath)
  )
    throw new YasProtocolError("invalid Git diff metadata");
  return new YasWriter()
    .u8(value.status)
    .u8(value.similarityPercent)
    .u16(value.flags)
    .bytesU32(value.oldPath ? encodeFsPath(value.oldPath) : new Uint8Array())
    .bytesU32(value.newPath ? encodeFsPath(value.newPath) : new Uint8Array())
    .u32(value.oldMode)
    .u32(value.newMode)
    .u8(value.oldObject ? 1 : 0)
    .u8(value.newObject ? 1 : 0)
    .u16(0)
    .bytes(
      value.oldObject ? encodeGitObjectId(value.oldObject) : new Uint8Array(),
    )
    .bytes(
      value.newObject ? encodeGitObjectId(value.newObject) : new Uint8Array(),
    )
    .finish();
}

export function decodeGitDiffRecord(bytes: Uint8Array): YasGitDiffRecord {
  const cursor = new YasCursor(bytes);
  const status = cursor.u8("Git diff status");
  const similarityPercent = cursor.u8("Git similarity");
  const flags = cursor.u16("Git diff flags");
  const oldPath = cursor.bytesU32("Git old path");
  const newPath = cursor.bytesU32("Git new path");
  const oldMode = cursor.u32("Git old mode");
  const newMode = cursor.u32("Git new mode");
  const oldPresent = cursor.u8("Git old object presence");
  const newPresent = cursor.u8("Git new object presence");
  if (oldPresent > 1 || newPresent > 1 || cursor.u16("Git diff reserved") !== 0)
    throw new YasProtocolError("invalid Git object presence");
  const value: YasGitDiffRecord = {
    kind: "diff",
    status,
    similarityPercent,
    flags,
    oldPath: oldPath.length ? decodeFsPath(oldPath) : undefined,
    newPath: newPath.length ? decodeFsPath(newPath) : undefined,
    oldMode,
    newMode,
    oldObject: oldPresent ? decodeObjectId(cursor) : undefined,
    newObject: newPresent ? decodeObjectId(cursor) : undefined,
  };
  cursor.end("Git diff");
  encodeGitDiffRecord(value);
  return value;
}

export function encodeGitPatchFileRecord(
  value: YasGitPatchFileRecord,
): Uint8Array {
  validatePatchPaths(value.status, value.oldPath, value.newPath);
  if (
    value.similarityPercent > 100 ||
    value.flags & ~g.YAS_GIT_PATCH_FILE_FLAGS
  )
    throw new YasProtocolError("invalid Git patch file metadata");
  return new YasWriter()
    .u8(value.status)
    .u8(value.similarityPercent)
    .u16(value.flags)
    .bytesU32(value.oldPath ? encodeFsPath(value.oldPath) : new Uint8Array())
    .bytesU32(value.newPath ? encodeFsPath(value.newPath) : new Uint8Array())
    .finish();
}

export function decodeGitPatchFileRecord(
  bytes: Uint8Array,
): YasGitPatchFileRecord {
  const cursor = new YasCursor(bytes);
  const status = cursor.u8("Git patch file status");
  const similarityPercent = cursor.u8("Git patch file similarity");
  const flags = cursor.u16("Git patch file flags");
  const oldPath = cursor.bytesU32("Git patch old path");
  const newPath = cursor.bytesU32("Git patch new path");
  const value: YasGitPatchFileRecord = {
    kind: "patch-file",
    status,
    similarityPercent,
    flags,
    oldPath: oldPath.length ? decodeFsPath(oldPath) : undefined,
    newPath: newPath.length ? decodeFsPath(newPath) : undefined,
  };
  cursor.end("Git patch file record");
  encodeGitPatchFileRecord(value);
  return value;
}

export function encodeGitPatchRowRecord(
  value: YasGitPatchRowRecord,
): Uint8Array {
  validatePatchRow(value);
  const writer = new YasWriter()
    .u32(value.oldLine)
    .u32(value.newLine)
    .bytesU32(value.oldText)
    .bytesU32(value.newText);
  encodePatchSpans(writer, value.oldSpans);
  encodePatchSpans(writer, value.newSpans);
  return writer.finish();
}

export function decodeGitPatchRowRecord(
  bytes: Uint8Array,
): YasGitPatchRowRecord {
  const cursor = new YasCursor(bytes);
  const oldLine = cursor.u32("Git patch old line");
  const newLine = cursor.u32("Git patch new line");
  const oldText = new Uint8Array(cursor.bytesU32("Git patch old text"));
  const newText = new Uint8Array(cursor.bytesU32("Git patch new text"));
  const value: YasGitPatchRowRecord = {
    kind: "patch-row",
    oldLine,
    newLine,
    oldText,
    newText,
    oldSpans: decodePatchSpans(cursor, "old"),
    newSpans: decodePatchSpans(cursor, "new"),
  };
  cursor.end("Git patch row record");
  encodeGitPatchRowRecord(value);
  return value;
}

export function encodeGitPatchGapRecord(
  value: YasGitPatchGapRecord,
): Uint8Array {
  if (value.oldLine === 0 && value.newLine === 0)
    throw new YasProtocolError("empty Git patch gap");
  return new YasWriter().u32(value.oldLine).u32(value.newLine).finish();
}

export function decodeGitPatchGapRecord(
  bytes: Uint8Array,
): YasGitPatchGapRecord {
  const cursor = new YasCursor(bytes);
  const value: YasGitPatchGapRecord = {
    kind: "patch-gap",
    oldLine: cursor.u32("Git patch gap old line"),
    newLine: cursor.u32("Git patch gap new line"),
  };
  cursor.end("Git patch gap record");
  encodeGitPatchGapRecord(value);
  return value;
}

export function encodeGitPatchBaseRecord(
  value: YasGitPatchBaseRecord,
): Uint8Array {
  return encodeGitObjectId(value.object);
}

export function decodeGitPatchBaseRecord(
  bytes: Uint8Array,
): YasGitPatchBaseRecord {
  const cursor = new YasCursor(bytes);
  const value: YasGitPatchBaseRecord = {
    kind: "patch-base",
    object: decodeObjectId(cursor),
  };
  cursor.end("Git patch base record");
  return value;
}

export function encodeGitIndexEntryRecord(
  value: YasGitIndexEntryRecord,
): Uint8Array {
  if (
    value.stage > 3 ||
    value.status > g.YAS_GIT_INDEX_STATUS_DELETED ||
    value.flags & ~g.YAS_GIT_INDEX_ENTRY_FLAGS
  )
    throw new YasProtocolError("invalid Git index entry");
  requireNonRootPath(value.path);
  return new YasWriter()
    .u8(value.stage)
    .u8(value.status)
    .u16(value.flags)
    .bytesU32(encodeFsPath(value.path))
    .u32(value.mode)
    .u64(value.size)
    .i64(value.modifiedUnixNs)
    .bytes(encodeGitObjectId(value.object))
    .finish();
}

export function decodeGitIndexEntryRecord(
  bytes: Uint8Array,
): YasGitIndexEntryRecord {
  const cursor = new YasCursor(bytes);
  const stage = cursor.u8("Git index stage");
  const status = cursor.u8("Git index status");
  const flags = cursor.u16("Git index flags");
  const path = decodeFsPath(cursor.bytesU32("Git index path"));
  const mode = cursor.u32("Git index mode");
  const value: YasGitIndexEntryRecord = {
    kind: "index-entry",
    stage,
    status,
    flags,
    path,
    mode,
    size: cursor.u64("Git index size"),
    modifiedUnixNs: cursor.i64("Git index modification time"),
    object: decodeObjectId(cursor),
  };
  cursor.end("Git index entry");
  encodeGitIndexEntryRecord(value);
  return value;
}

export function encodeGitDiscoveryRecord(
  value: YasGitDiscoveryRecord,
): Uint8Array {
  if (
    value.flags & ~g.YAS_GIT_DISCOVERY_FLAGS ||
    value.objectAlgorithm > g.YAS_GIT_OBJECT_SHA256
  )
    throw new YasProtocolError("invalid Git discovery metadata");
  const bare = Boolean(value.flags & g.YAS_GIT_DISCOVERY_BARE);
  if (bare !== (value.worktreePath.length === 0))
    throw new YasProtocolError("invalid Git discovery worktree path");
  if (!bare)
    validatePlatformPath(value.worktreePath, "Git discovery worktree path");
  validatePlatformPath(value.gitDir, "Git discovery Git dir");
  return new YasWriter()
    .u16(value.flags)
    .u16(0)
    .u8(value.objectAlgorithm)
    .bytes(new Uint8Array(3))
    .bytesU32(value.worktreePath)
    .bytesU32(value.gitDir)
    .finish();
}

export function decodeGitDiscoveryRecord(
  bytes: Uint8Array,
): YasGitDiscoveryRecord {
  const cursor = new YasCursor(bytes);
  const flags = cursor.u16("Git discovery flags");
  if (cursor.u16("Git discovery reserved") !== 0)
    throw new YasProtocolError("Git discovery reserved field is nonzero");
  const objectAlgorithm = cursor.u8("Git object algorithm");
  requireZero(cursor.take(3, "Git discovery reserved"), "Git discovery");
  const value: YasGitDiscoveryRecord = {
    kind: "discovery",
    flags,
    objectAlgorithm,
    worktreePath: new Uint8Array(
      cursor.bytesU32("Git discovery worktree path"),
    ),
    gitDir: new Uint8Array(cursor.bytesU32("Git discovery Git dir")),
  };
  cursor.end("Git discovery");
  encodeGitDiscoveryRecord(value);
  return value;
}

export function encodeGitBlameRecord(value: YasGitBlameRecord): Uint8Array {
  if (
    value.flags & ~g.YAS_GIT_BLAME_RECORD_FLAGS ||
    value.startLine === 0 ||
    value.startLine >= value.endLine ||
    value.originalStartLine === 0
  )
    throw new YasProtocolError("invalid Git blame range");
  if (value.originalPath) requireNonRootPath(value.originalPath);
  validateOptionalRaw(
    value.author,
    g.YAS_GIT_MAX_IDENTITY_BYTES,
    false,
    "Git author",
  );
  validateOptionalRaw(
    value.summary,
    g.YAS_GIT_MAX_SUMMARY_BYTES,
    false,
    "Git summary",
  );
  return new YasWriter()
    .u16(value.flags)
    .u16(0)
    .u32(value.startLine)
    .u32(value.endLine)
    .u32(value.originalStartLine)
    .bytes(encodeGitObjectId(value.commit))
    .bytesU32(
      value.originalPath ? encodeFsPath(value.originalPath) : new Uint8Array(),
    )
    .bytesU16(value.author)
    .bytesU16(value.summary)
    .finish();
}

export function decodeGitBlameRecord(bytes: Uint8Array): YasGitBlameRecord {
  const cursor = new YasCursor(bytes);
  const flags = cursor.u16("Git blame flags");
  if (cursor.u16("Git blame reserved") !== 0)
    throw new YasProtocolError("Git blame reserved field is nonzero");
  const startLine = cursor.u32("Git blame start line");
  const endLine = cursor.u32("Git blame end line");
  const originalStartLine = cursor.u32("Git blame original start line");
  const commit = decodeObjectId(cursor);
  const originalPath = cursor.bytesU32("Git blame original path");
  const value: YasGitBlameRecord = {
    kind: "blame",
    flags,
    startLine,
    endLine,
    originalStartLine,
    commit,
    originalPath: originalPath.length ? decodeFsPath(originalPath) : undefined,
    author: new Uint8Array(cursor.bytesU16("Git blame author")),
    summary: new Uint8Array(cursor.bytesU16("Git blame summary")),
  };
  cursor.end("Git blame");
  encodeGitBlameRecord(value);
  return value;
}

export function encodeGitReflogRecord(value: YasGitReflogRecord): Uint8Array {
  validateRaw(
    value.committer,
    g.YAS_GIT_MAX_IDENTITY_BYTES,
    false,
    "Git committer",
  );
  if (value.message.length > g.YAS_GIT_MAX_MESSAGE_BYTES)
    throw new YasProtocolError("Git reflog message exceeds limit");
  if (value.flags & ~g.YAS_GIT_REFLOG_RECORD_FLAGS)
    throw new YasProtocolError("invalid Git reflog flags");
  return new YasWriter()
    .u16(value.flags)
    .u16(0)
    .u64(value.index)
    .bytes(encodeGitObjectId(value.oldObject))
    .bytes(encodeGitObjectId(value.newObject))
    .bytesU16(value.committer)
    .i64(value.committedUnixSeconds)
    .i16(value.timezoneMinutes)
    .u16(0)
    .bytesU32(value.message)
    .finish();
}

export function decodeGitReflogRecord(bytes: Uint8Array): YasGitReflogRecord {
  const cursor = new YasCursor(bytes);
  const flags = cursor.u16("Git reflog flags");
  if (cursor.u16("Git reflog reserved") !== 0)
    throw new YasProtocolError("Git reflog reserved field is nonzero");
  const index = cursor.u64("Git reflog index");
  const oldObject = decodeObjectId(cursor);
  const newObject = decodeObjectId(cursor);
  const committer = new Uint8Array(cursor.bytesU16("Git reflog committer"));
  const committedUnixSeconds = cursor.i64("Git reflog time");
  const timezoneMinutes = cursor.i16("Git reflog timezone");
  if (cursor.u16("Git reflog timezone reserved") !== 0)
    throw new YasProtocolError("Git reflog timezone reserved field is nonzero");
  const value: YasGitReflogRecord = {
    kind: "reflog",
    flags,
    index,
    oldObject,
    newObject,
    committer,
    committedUnixSeconds,
    timezoneMinutes,
    message: new Uint8Array(cursor.bytesU32("Git reflog message")),
  };
  cursor.end("Git reflog");
  encodeGitReflogRecord(value);
  return value;
}

export function encodeGitWorktreeRecord(
  value: YasGitWorktreeRecord,
): Uint8Array {
  if (value.flags & ~g.YAS_GIT_WORKTREE_FLAGS)
    throw new YasProtocolError("invalid Git worktree flags");
  const bare = Boolean(value.flags & g.YAS_GIT_WORKTREE_BARE);
  const detached = Boolean(value.flags & g.YAS_GIT_WORKTREE_DETACHED);
  const locked = Boolean(value.flags & g.YAS_GIT_WORKTREE_LOCKED);
  if (
    bare !== (value.path.length === 0) ||
    (bare || detached) !== (value.branch.length === 0) ||
    (!locked && value.lockReason.length !== 0) ||
    value.branch.length > g.YAS_GIT_MAX_SPEC_BYTES ||
    value.branch.includes(0) ||
    new TextEncoder().encode(value.lockReason).length >
      g.YAS_GIT_MAX_SUMMARY_BYTES ||
    value.lockReason.includes("\0")
  )
    throw new YasProtocolError("invalid Git worktree metadata");
  if (!bare) validatePlatformPath(value.path, "Git worktree path");
  return new YasWriter()
    .u16(value.flags)
    .u16(0)
    .bytesU32(value.path)
    .u8(value.head ? 1 : 0)
    .bytes(new Uint8Array(3))
    .bytes(value.head ? encodeGitObjectId(value.head) : new Uint8Array())
    .bytesU16(value.branch)
    .utf8U16(value.lockReason)
    .finish();
}

export function decodeGitWorktreeRecord(
  bytes: Uint8Array,
): YasGitWorktreeRecord {
  const cursor = new YasCursor(bytes);
  const flags = cursor.u16("Git worktree flags");
  if (cursor.u16("Git worktree reserved") !== 0)
    throw new YasProtocolError("Git worktree reserved field is nonzero");
  const path = new Uint8Array(cursor.bytesU32("Git worktree path"));
  const headPresent = cursor.u8("Git worktree HEAD presence");
  requireZero(cursor.take(3, "Git worktree reserved"), "Git worktree");
  if (headPresent > 1)
    throw new YasProtocolError("invalid Git worktree HEAD presence");
  const value: YasGitWorktreeRecord = {
    kind: "worktree",
    flags,
    path,
    head: headPresent ? decodeObjectId(cursor) : undefined,
    branch: new Uint8Array(cursor.bytesU16("Git worktree branch")),
    lockReason: cursor.utf8U16("Git worktree lock reason"),
  };
  cursor.end("Git worktree");
  encodeGitWorktreeRecord(value);
  return value;
}

export function encodeGitObjectRecord(value: YasGitObjectRecord): Uint8Array {
  if (value.role > g.YAS_GIT_OBJECT_ROLE_HIDE)
    throw new YasProtocolError("invalid Git object result role");
  return new YasWriter()
    .u8(value.role)
    .bytes(new Uint8Array(3))
    .bytes(encodeGitObjectId(value.object))
    .finish();
}

export function decodeGitObjectRecord(bytes: Uint8Array): YasGitObjectRecord {
  const cursor = new YasCursor(bytes);
  const role = cursor.u8("Git object result role");
  requireZero(
    cursor.take(3, "Git object result reserved"),
    "Git object result",
  );
  const value: YasGitObjectRecord = {
    kind: "object",
    role,
    object: decodeObjectId(cursor),
  };
  cursor.end("Git object result");
  encodeGitObjectRecord(value);
  return value;
}

export function encodeGitQueryRecord(value: YasGitQueryRecord): Uint8Array {
  let kind: number;
  let body: Uint8Array;
  if (value.kind === "object") {
    kind = g.YAS_GIT_RESULT_OBJECT;
    body = encodeGitObjectRecord(value);
  } else if (value.kind === "commit") {
    kind = g.YAS_GIT_RESULT_COMMIT;
    body = encodeGitCommitRecord(value);
  } else if (value.kind === "log-path") {
    kind = g.YAS_GIT_RESULT_LOG_PATH;
    body = encodeGitLogPathRecord(value);
  } else if (value.kind === "tree-entry") {
    kind = g.YAS_GIT_RESULT_TREE_ENTRY;
    body = encodeGitTreeEntryRecord(value);
  } else if (value.kind === "blob") {
    kind = g.YAS_GIT_RESULT_BLOB;
    body = encodeGitContentRecord(value);
  } else if (value.kind === "diff") {
    kind = g.YAS_GIT_RESULT_DIFF;
    body = encodeGitDiffRecord(value);
  } else if (value.kind === "patch-file") {
    kind = g.YAS_GIT_RESULT_PATCH_FILE;
    body = encodeGitPatchFileRecord(value);
  } else if (value.kind === "patch-row") {
    kind = g.YAS_GIT_RESULT_PATCH_ROW;
    body = encodeGitPatchRowRecord(value);
  } else if (value.kind === "patch-gap") {
    kind = g.YAS_GIT_RESULT_PATCH_GAP;
    body = encodeGitPatchGapRecord(value);
  } else if (value.kind === "patch-base") {
    kind = g.YAS_GIT_RESULT_PATCH_BASE;
    body = encodeGitPatchBaseRecord(value);
  } else if (value.kind === "patch") {
    kind = g.YAS_GIT_RESULT_PATCH;
    body = encodeGitContentRecord(value);
  } else if (value.kind === "index-entry") {
    kind = g.YAS_GIT_RESULT_INDEX_ENTRY;
    body = encodeGitIndexEntryRecord(value);
  } else if (value.kind === "discovery") {
    kind = g.YAS_GIT_RESULT_DISCOVERY;
    body = encodeGitDiscoveryRecord(value);
  } else if (value.kind === "blame") {
    kind = g.YAS_GIT_RESULT_BLAME;
    body = encodeGitBlameRecord(value);
  } else if (value.kind === "reflog") {
    kind = g.YAS_GIT_RESULT_REFLOG;
    body = encodeGitReflogRecord(value);
  } else if (value.kind === "worktree") {
    kind = g.YAS_GIT_RESULT_WORKTREE;
    body = encodeGitWorktreeRecord(value);
  } else throw new YasProtocolError("unknown Git query record kind");
  return encodeTypedRecord({ kind, flags: 0, body });
}

export function decodeGitQueryRecord(
  cursor: YasCursor,
): YasGitQueryRecord | undefined {
  const length = cursor.u32("Git query record length");
  const record = cursor.sub(length, "Git query record");
  const kind = record.u16("Git query record kind");
  const flags = record.u16("Git query record flags");
  if (flags & ~1) throw new YasProtocolError("invalid Git query record flags");
  const body = new Uint8Array(record.take(record.remaining));
  if (kind === g.YAS_GIT_RESULT_OBJECT) return decodeGitObjectRecord(body);
  if (kind === g.YAS_GIT_RESULT_COMMIT) return decodeGitCommitRecord(body);
  if (kind === g.YAS_GIT_RESULT_LOG_PATH) return decodeGitLogPathRecord(body);
  if (kind === g.YAS_GIT_RESULT_TREE_ENTRY)
    return decodeGitTreeEntryRecord(body);
  if (kind === g.YAS_GIT_RESULT_BLOB)
    return decodeGitContentRecord(body, "blob");
  if (kind === g.YAS_GIT_RESULT_DIFF) return decodeGitDiffRecord(body);
  if (kind === g.YAS_GIT_RESULT_PATCH_FILE)
    return decodeGitPatchFileRecord(body);
  if (kind === g.YAS_GIT_RESULT_PATCH_ROW) return decodeGitPatchRowRecord(body);
  if (kind === g.YAS_GIT_RESULT_PATCH_GAP) return decodeGitPatchGapRecord(body);
  if (kind === g.YAS_GIT_RESULT_PATCH_BASE)
    return decodeGitPatchBaseRecord(body);
  if (kind === g.YAS_GIT_RESULT_PATCH)
    return decodeGitContentRecord(body, "patch");
  if (kind === g.YAS_GIT_RESULT_INDEX_ENTRY)
    return decodeGitIndexEntryRecord(body);
  if (kind === g.YAS_GIT_RESULT_DISCOVERY)
    return decodeGitDiscoveryRecord(body);
  if (kind === g.YAS_GIT_RESULT_BLAME) return decodeGitBlameRecord(body);
  if (kind === g.YAS_GIT_RESULT_REFLOG) return decodeGitReflogRecord(body);
  if (kind === g.YAS_GIT_RESULT_WORKTREE) return decodeGitWorktreeRecord(body);
  if (flags & 1)
    throw new YasProtocolError("unknown required Git query record");
  return undefined;
}

export function encodeGitQueryPage(value: YasGitQueryPageWire): Uint8Array {
  const nextCursor = encodeGitQueryCursor(value.nextCursor);
  if (
    value.flags & ~g.YAS_GIT_QUERY_PAGE_FLAGS ||
    Boolean(value.flags & g.YAS_GIT_QUERY_PAGE_MORE) !==
      (value.nextCursor.kind !== "start")
  )
    throw new YasProtocolError("invalid Git query page flags");
  rejectRequired(value.extensions, "Git query page");
  const writer = new YasWriter()
    .bytesU16(nextCursor)
    .u64(value.totalHint)
    .u16(value.flags)
    .u16(0)
    .u8(
      value.delivery.kind === "inline"
        ? g.YAS_GIT_PAGE_INLINE
        : g.YAS_GIT_PAGE_TRANSFER,
    )
    .bytes(new Uint8Array(3));
  if (value.delivery.kind === "inline") {
    if (value.delivery.records.length > g.YAS_GIT_MAX_QUERY_RECORDS)
      throw new YasProtocolError("too many Git query records");
    const records = new YasWriter();
    for (const record of value.delivery.records)
      records.bytes(encodeGitQueryRecord(record));
    const bytes = records.finish();
    if (bytes.length > g.YAS_GIT_MAX_QUERY_BYTES)
      throw new YasProtocolError("Git query page exceeds byte limit");
    writer.u16(value.delivery.records.length).u16(0).bytesU32(bytes);
  } else {
    validatePageDescriptor(value.delivery.descriptor);
    writer.bytesU32(encodeTransferDescriptor(value.delivery.descriptor));
  }
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeGitQueryPage(bytes: Uint8Array): YasGitQueryPageWire {
  const cursor = new YasCursor(bytes);
  const nextCursor = decodeGitQueryCursor(cursor.bytesU16("Git query cursor"));
  const totalHint = cursor.u64("Git query total hint");
  const flags = cursor.u16("Git query page flags");
  if (cursor.u16("Git query page reserved") !== 0)
    throw new YasProtocolError("Git query page reserved field is nonzero");
  const deliveryKind = cursor.u8("Git page delivery");
  requireZero(cursor.take(3, "Git page reserved"), "Git page");
  let delivery: YasGitPageDelivery;
  if (deliveryKind === g.YAS_GIT_PAGE_INLINE) {
    const count = cursor.u16("Git page record count");
    if (
      cursor.u16("Git page reserved") !== 0 ||
      count > g.YAS_GIT_MAX_QUERY_RECORDS
    )
      throw new YasProtocolError("invalid Git page record count");
    const recordBytes = cursor.bytesU32("Git record stream");
    if (
      recordBytes.length > g.YAS_GIT_MAX_QUERY_BYTES ||
      count > Math.floor(recordBytes.length / 8)
    )
      throw new YasProtocolError("invalid Git record stream");
    const recordsCursor = new YasCursor(recordBytes);
    const records: YasGitQueryRecord[] = [];
    for (let index = 0; index < count; index++) {
      const record = decodeGitQueryRecord(recordsCursor);
      if (record) records.push(record);
    }
    recordsCursor.end("Git query records");
    delivery = { kind: "inline", records };
  } else if (deliveryKind === g.YAS_GIT_PAGE_TRANSFER) {
    const descriptorCursor = new YasCursor(
      cursor.bytesU32("Git query Transfer descriptor"),
    );
    const descriptor = decodeTransferDescriptor(descriptorCursor);
    descriptorCursor.end("Git query Transfer descriptor");
    validatePageDescriptor(descriptor);
    delivery = { kind: "transfer", descriptor };
  } else throw new YasProtocolError("unknown Git page delivery");
  const value = {
    nextCursor,
    totalHint,
    flags,
    delivery,
    extensions: decodeExtensions(cursor, new Set(), "Git page extensions"),
  };
  cursor.end("Git query page");
  encodeGitQueryPage(value);
  return value;
}

export function encodeGitFetch(value: YasGitFetch): Uint8Array {
  requireHandle(value.repositoryHandle, "Git repository handle");
  requireOperationId(value.operationId);
  if (
    value.flags & ~g.YAS_GIT_FETCH_FLAGS ||
    value.remote.length > g.YAS_GIT_MAX_REMOTE_BYTES ||
    value.remote.includes(0) ||
    value.refspecs.length > g.YAS_GIT_MAX_REFSPECS
  )
    throw new YasProtocolError("invalid Git FETCH metadata");
  const unique = new Set<string>();
  for (const refspec of value.refspecs) {
    validateSpec(refspec);
    const key = hex(refspec);
    if (unique.has(key)) throw new YasProtocolError("duplicate Git refspec");
    unique.add(key);
  }
  rejectRequired(value.extensions, "Git FETCH");
  const writer = new YasWriter()
    .u64(value.repositoryHandle)
    .bytes(value.operationId)
    .u16(value.flags)
    .u16(value.refspecs.length)
    .u32(value.timeoutMs)
    .bytesU16(value.remote);
  for (const refspec of value.refspecs) writer.bytesU16(refspec);
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeGitFetch(bytes: Uint8Array): YasGitFetch {
  const cursor = new YasCursor(bytes);
  const repositoryHandle = cursor.u64("Git repository handle");
  const operationId = new Uint8Array(cursor.take(16, "Git operation ID"));
  const flags = cursor.u16("Git FETCH flags");
  const count = cursor.u16("Git refspec count");
  const timeoutMs = cursor.u32("Git FETCH timeout");
  const remote = new Uint8Array(cursor.bytesU16("Git remote"));
  if (
    count > g.YAS_GIT_MAX_REFSPECS ||
    count > Math.floor(cursor.remaining / 2)
  )
    throw new YasProtocolError("invalid Git refspec count");
  const refspecs: Uint8Array[] = [];
  for (let index = 0; index < count; index++)
    refspecs.push(new Uint8Array(cursor.bytesU16("Git refspec")));
  const value = {
    repositoryHandle,
    operationId,
    flags,
    timeoutMs,
    remote,
    refspecs,
    extensions: decodeExtensions(cursor, new Set(), "Git FETCH extensions"),
  };
  cursor.end("Git FETCH");
  encodeGitFetch(value);
  return value;
}

export function encodeGitFetchResult(value: YasGitFetchResult): Uint8Array {
  requireRevision(value.repositoryRevision, "Git repository revision");
  if (value.refs.length > g.YAS_GIT_MAX_REFSPECS)
    throw new YasProtocolError("too many Git FETCH ref results");
  rejectRequired(value.extensions, "Git FETCH Result");
  const writer = new YasWriter()
    .u64(value.repositoryRevision)
    .u16(value.refs.length)
    .u16(0);
  for (const ref of value.refs) writer.bytesU32(encodeGitFetchRefResult(ref));
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeGitFetchResult(bytes: Uint8Array): YasGitFetchResult {
  const cursor = new YasCursor(bytes);
  const repositoryRevision = cursor.u64("Git repository revision");
  const count = cursor.u16("Git FETCH ref result count");
  if (
    cursor.u16("Git FETCH Result reserved") !== 0 ||
    count > g.YAS_GIT_MAX_REFSPECS ||
    count > cursor.remaining / 4
  )
    throw new YasProtocolError("Git FETCH Result reserved is nonzero");
  const refs: YasGitFetchRefResult[] = [];
  for (let index = 0; index < count; index++)
    refs.push(decodeGitFetchRefResult(cursor.bytesU32("Git FETCH ref result")));
  const value = {
    repositoryRevision,
    refs,
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Git FETCH Result extensions",
    ),
  };
  cursor.end("Git FETCH Result");
  encodeGitFetchResult(value);
  return value;
}

export function encodeGitFetchRefResult(
  value: YasGitFetchRefResult,
): Uint8Array {
  const pruned = Boolean(value.flags & g.YAS_GIT_FETCH_REF_PRUNED);
  const newRef = Boolean(value.flags & g.YAS_GIT_FETCH_REF_NEW);
  if (
    value.flags & ~g.YAS_GIT_FETCH_REF_FLAGS ||
    (pruned && value.new) ||
    (newRef && value.old) ||
    value.status > g.YAS_STATUS_INTERNAL ||
    new TextEncoder().encode(value.detail).length >
      g.YAS_GIT_MAX_SUMMARY_BYTES ||
    value.detail.includes("\0")
  )
    throw new YasProtocolError("invalid Git FETCH ref result");
  validateSpec(value.name);
  return new YasWriter()
    .u16(value.flags)
    .u16(value.status)
    .u8(value.old ? 1 : 0)
    .u8(value.new ? 1 : 0)
    .u16(0)
    .bytes(value.old ? encodeGitObjectId(value.old) : new Uint8Array())
    .bytes(value.new ? encodeGitObjectId(value.new) : new Uint8Array())
    .bytesU16(value.name)
    .utf8U16(value.detail)
    .finish();
}

export function decodeGitFetchRefResult(
  bytes: Uint8Array,
): YasGitFetchRefResult {
  const cursor = new YasCursor(bytes);
  const flags = cursor.u16("Git FETCH ref flags");
  const status = cursor.u16("Git FETCH ref status");
  const oldPresent = cursor.u8("Git FETCH old object presence");
  const newPresent = cursor.u8("Git FETCH new object presence");
  if (
    oldPresent > 1 ||
    newPresent > 1 ||
    cursor.u16("Git FETCH ref reserved") !== 0
  )
    throw new YasProtocolError("invalid Git FETCH ref presence");
  const value = {
    flags,
    status,
    old: oldPresent ? decodeObjectId(cursor) : undefined,
    new: newPresent ? decodeObjectId(cursor) : undefined,
    name: new Uint8Array(cursor.bytesU16("Git FETCH ref name")),
    detail: cursor.utf8U16("Git FETCH ref detail"),
  };
  cursor.end("Git FETCH ref result");
  encodeGitFetchRefResult(value);
  return value;
}

export function encodeGitProgress(value: YasGitProgress): Uint8Array {
  requireOperationId(value.operationId);
  if (
    value.phase > g.YAS_GIT_PROGRESS_UPDATE_REFS ||
    value.flags & ~g.YAS_GIT_PROGRESS_FLAGS ||
    new TextEncoder().encode(value.message).length >
      g.YAS_GIT_MAX_PROGRESS_MESSAGE_BYTES
  )
    throw new YasProtocolError("invalid Git progress");
  return new YasWriter()
    .bytes(value.operationId)
    .u8(value.phase)
    .u8(value.flags)
    .u16(0)
    .u64(value.current)
    .u64(value.total)
    .utf8U16(value.message)
    .finish();
}

export function decodeGitProgress(bytes: Uint8Array): YasGitProgress {
  const cursor = new YasCursor(bytes);
  const operationId = new Uint8Array(cursor.take(16, "Git operation ID"));
  const phase = cursor.u8("Git progress phase");
  const flags = cursor.u8("Git progress flags");
  if (cursor.u16("Git progress reserved") !== 0)
    throw new YasProtocolError("Git progress reserved field is nonzero");
  const value = {
    operationId,
    phase,
    flags,
    current: cursor.u64("Git progress current"),
    total: cursor.u64("Git progress total"),
    message: cursor.utf8U16("Git progress message"),
  };
  cursor.end("Git progress");
  encodeGitProgress(value);
  return value;
}

export function encodeGitClosed(value: YasGitClosed): Uint8Array {
  requireHandle(value.repositoryHandle, "Git repository handle");
  requireRevision(value.repositoryRevision, "Git CLOSED repository revision");
  if (
    value.reason > g.YAS_GIT_CLOSED_RESOURCE_LIMIT ||
    new TextEncoder().encode(value.detail).length >
      g.YAS_GIT_MAX_SUMMARY_BYTES ||
    value.detail.includes("\0")
  )
    throw new YasProtocolError("invalid Git CLOSED event");
  return new YasWriter()
    .u64(value.repositoryHandle)
    .u64(value.repositoryRevision)
    .u8(value.reason)
    .bytes(new Uint8Array(3))
    .utf8U16(value.detail)
    .finish();
}

export function decodeGitClosed(bytes: Uint8Array): YasGitClosed {
  const cursor = new YasCursor(bytes);
  const value: YasGitClosed = {
    repositoryHandle: cursor.u64("Git CLOSED repository handle"),
    repositoryRevision: cursor.u64("Git CLOSED repository revision"),
    reason: cursor.u8("Git CLOSED reason"),
    detail: "",
  };
  requireZero(cursor.take(3, "Git CLOSED reserved"), "Git CLOSED");
  value.detail = cursor.utf8U16("Git CLOSED detail");
  cursor.end("Git CLOSED");
  encodeGitClosed(value);
  return value;
}

export function encodeGitHeadEntityBody(
  value: YasGitHeadEntityBody,
): Uint8Array {
  validateHeadEntityBody(value);
  return new YasWriter()
    .u16(value.flags)
    .u16(0)
    .u8(value.object ? 1 : 0)
    .bytes(new Uint8Array(3))
    .bytes(value.object ? encodeGitObjectId(value.object) : new Uint8Array())
    .bytesU16(value.symbolicTarget)
    .finish();
}

export function decodeGitHeadEntityBody(
  bytes: Uint8Array,
): YasGitHeadEntityBody {
  const cursor = new YasCursor(bytes);
  const flags = cursor.u16("Git HEAD flags");
  if (cursor.u16("Git HEAD reserved") !== 0)
    throw new YasProtocolError("Git HEAD reserved field is nonzero");
  const present = cursor.u8("Git HEAD object presence");
  requireZero(cursor.take(3, "Git HEAD reserved"), "Git HEAD");
  if (present > 1)
    throw new YasProtocolError("invalid Git HEAD object presence");
  const value: YasGitHeadEntityBody = {
    kind: "head",
    flags,
    object: present ? decodeObjectId(cursor) : undefined,
    symbolicTarget: new Uint8Array(cursor.bytesU16("Git symbolic target")),
  };
  cursor.end("Git HEAD entity");
  validateHeadEntityBody(value);
  return value;
}

export function encodeGitRefEntityBody(value: YasGitRefEntityBody): Uint8Array {
  validateRefEntityBody(value);
  return new YasWriter()
    .u16(value.flags)
    .u16(0)
    .bytes(encodeGitObjectId(value.object))
    .u8(value.peeled ? 1 : 0)
    .bytes(new Uint8Array(3))
    .bytes(value.peeled ? encodeGitObjectId(value.peeled) : new Uint8Array())
    .bytesU16(value.symbolicTarget)
    .finish();
}

export function decodeGitRefEntityBody(bytes: Uint8Array): YasGitRefEntityBody {
  const cursor = new YasCursor(bytes);
  const flags = cursor.u16("Git ref flags");
  if (cursor.u16("Git ref reserved") !== 0)
    throw new YasProtocolError("Git ref reserved field is nonzero");
  const object = decodeObjectId(cursor);
  const present = cursor.u8("Git peeled object presence");
  requireZero(cursor.take(3, "Git ref reserved"), "Git ref");
  if (present > 1)
    throw new YasProtocolError("invalid Git peeled object presence");
  const value: YasGitRefEntityBody = {
    kind: "ref",
    flags,
    object,
    peeled: present ? decodeObjectId(cursor) : undefined,
    symbolicTarget: new Uint8Array(cursor.bytesU16("Git symbolic target")),
  };
  cursor.end("Git ref entity");
  validateRefEntityBody(value);
  return value;
}

export function encodeGitRemoteEntityBody(
  value: YasGitRemoteEntityBody,
): Uint8Array {
  validateRemoteEntityBody(value);
  return new YasWriter()
    .u16(value.flags)
    .u16(0)
    .bytesU32(value.fetchUrl)
    .bytesU32(value.pushUrl)
    .finish();
}

export function decodeGitRemoteEntityBody(
  bytes: Uint8Array,
): YasGitRemoteEntityBody {
  const cursor = new YasCursor(bytes);
  const flags = cursor.u16("Git remote flags");
  if (cursor.u16("Git remote reserved") !== 0)
    throw new YasProtocolError("Git remote reserved field is nonzero");
  const value: YasGitRemoteEntityBody = {
    kind: "remote",
    flags,
    fetchUrl: new Uint8Array(cursor.bytesU32("Git fetch URL")),
    pushUrl: new Uint8Array(cursor.bytesU32("Git push URL")),
  };
  cursor.end("Git remote entity");
  validateRemoteEntityBody(value);
  return value;
}

export function encodeGitOperationEntityBody(
  value: YasGitOperationEntityBody,
): Uint8Array {
  validateOperationEntityBody(value);
  return new YasWriter()
    .u8(value.operationKind)
    .u8(value.flags)
    .u16(0)
    .u8(value.head ? 1 : 0)
    .bytes(new Uint8Array(3))
    .bytes(value.head ? encodeGitObjectId(value.head) : new Uint8Array())
    .utf8U16(value.detail)
    .finish();
}

export function decodeGitOperationEntityBody(
  bytes: Uint8Array,
): YasGitOperationEntityBody {
  const cursor = new YasCursor(bytes);
  const operationKind = cursor.u8("Git operation kind");
  const flags = cursor.u8("Git operation flags");
  if (cursor.u16("Git operation reserved") !== 0)
    throw new YasProtocolError("Git operation reserved field is nonzero");
  const present = cursor.u8("Git operation HEAD presence");
  requireZero(cursor.take(3, "Git operation reserved"), "Git operation");
  if (present > 1)
    throw new YasProtocolError("invalid Git operation HEAD presence");
  const value: YasGitOperationEntityBody = {
    kind: "operation",
    operationKind,
    flags,
    head: present ? decodeObjectId(cursor) : undefined,
    detail: cursor.utf8U16("Git operation detail"),
  };
  cursor.end("Git operation entity");
  validateOperationEntityBody(value);
  return value;
}

export function encodeGitStatusEntityBody(
  value: YasGitStatusEntityBody,
): Uint8Array {
  validateStatusEntityBody(value);
  return new YasWriter()
    .u8(value.indexStatus)
    .u8(value.worktreeStatus)
    .u16(value.flags)
    .u8(value.content ? 1 : 0)
    .u8(value.oldPath ? 1 : 0)
    .u16(0)
    .bytes(value.content ? encodeGitObjectId(value.content) : new Uint8Array())
    .bytes(
      value.oldPath
        ? new YasWriter().bytesU32(encodeFsPath(value.oldPath)).finish()
        : new Uint8Array(),
    )
    .finish();
}

export function decodeGitStatusEntityBody(
  bytes: Uint8Array,
): YasGitStatusEntityBody {
  const cursor = new YasCursor(bytes);
  const indexStatus = cursor.u8("Git index status");
  const worktreeStatus = cursor.u8("Git worktree status");
  const flags = cursor.u16("Git status flags");
  const contentPresent = cursor.u8("Git status content presence");
  const oldPathPresent = cursor.u8("Git status old-path presence");
  if (
    contentPresent > 1 ||
    oldPathPresent > 1 ||
    cursor.u16("Git status reserved") !== 0
  )
    throw new YasProtocolError("invalid Git status presence or reserved field");
  const value: YasGitStatusEntityBody = {
    kind: "status",
    indexStatus,
    worktreeStatus,
    flags,
    content: contentPresent ? decodeObjectId(cursor) : undefined,
    oldPath: oldPathPresent
      ? decodeFsPath(cursor.bytesU32("Git status old path"))
      : undefined,
  };
  cursor.end("Git status entity");
  validateStatusEntityBody(value);
  return value;
}

export function encodeGitUpstreamEntityBody(
  value: YasGitUpstreamEntityBody,
): Uint8Array {
  validateUpstreamEntityBody(value);
  return new YasWriter()
    .u16(value.flags)
    .u16(0)
    .u32(value.ahead)
    .u32(value.behind)
    .bytesU16(value.upstream)
    .finish();
}

export function decodeGitUpstreamEntityBody(
  bytes: Uint8Array,
): YasGitUpstreamEntityBody {
  const cursor = new YasCursor(bytes);
  const flags = cursor.u16("Git upstream flags");
  if (cursor.u16("Git upstream reserved") !== 0)
    throw new YasProtocolError("Git upstream reserved field is nonzero");
  const value: YasGitUpstreamEntityBody = {
    kind: "upstream",
    flags,
    ahead: cursor.u32("Git upstream ahead count"),
    behind: cursor.u32("Git upstream behind count"),
    upstream: new Uint8Array(cursor.bytesU16("Git upstream name")),
  };
  cursor.end("Git upstream entity");
  encodeGitUpstreamEntityBody(value);
  return value;
}

export function encodeGitStashEntityBody(
  value: YasGitStashEntityBody,
): Uint8Array {
  validateObjectId(value.object);
  if (value.message.length > g.YAS_GIT_MAX_MESSAGE_BYTES)
    throw new YasProtocolError("Git stash message exceeds limit");
  return new YasWriter()
    .bytes(encodeGitObjectId(value.object))
    .i64(value.createdUnixSeconds)
    .i16(value.timezoneMinutes)
    .u16(0)
    .bytesU32(value.message)
    .finish();
}

export function decodeGitStashEntityBody(
  bytes: Uint8Array,
): YasGitStashEntityBody {
  const cursor = new YasCursor(bytes);
  const object = decodeObjectId(cursor);
  const createdUnixSeconds = cursor.i64("Git stash creation time");
  const timezoneMinutes = cursor.i16("Git stash timezone");
  if (cursor.u16("Git stash reserved") !== 0)
    throw new YasProtocolError("Git stash reserved field is nonzero");
  const value: YasGitStashEntityBody = {
    kind: "stash",
    object,
    createdUnixSeconds,
    timezoneMinutes,
    message: new Uint8Array(cursor.bytesU32("Git stash message")),
  };
  cursor.end("Git stash entity");
  encodeGitStashEntityBody(value);
  return value;
}

export function encodeGitWorktreeGenerationEntityBody(
  value: YasGitWorktreeGenerationEntityBody,
): Uint8Array {
  return new YasWriter().u32(value.count).u32(0).u64(value.digest).finish();
}

export function decodeGitWorktreeGenerationEntityBody(
  bytes: Uint8Array,
): YasGitWorktreeGenerationEntityBody {
  const cursor = new YasCursor(bytes);
  const count = cursor.u32("Git worktree count");
  if (cursor.u32("Git worktree generation reserved") !== 0)
    throw new YasProtocolError(
      "Git worktree generation reserved field is nonzero",
    );
  const value: YasGitWorktreeGenerationEntityBody = {
    kind: "worktree-generation",
    count,
    digest: cursor.u64("Git worktree generation digest"),
  };
  cursor.end("Git worktree generation entity");
  return value;
}

export function encodeGitEntityBody(value: YasGitEntityBody): Uint8Array {
  if (value.kind === "head") return encodeGitHeadEntityBody(value);
  if (value.kind === "ref") return encodeGitRefEntityBody(value);
  if (value.kind === "remote") return encodeGitRemoteEntityBody(value);
  if (value.kind === "operation") return encodeGitOperationEntityBody(value);
  if (value.kind === "status") return encodeGitStatusEntityBody(value);
  if (value.kind === "upstream") return encodeGitUpstreamEntityBody(value);
  if (value.kind === "stash") return encodeGitStashEntityBody(value);
  return encodeGitWorktreeGenerationEntityBody(value);
}

export function decodeGitEntityBody(
  entityKind: number,
  bytes: Uint8Array,
): YasGitEntityBody {
  if (entityKind === g.YAS_GIT_ENTITY_HEAD)
    return decodeGitHeadEntityBody(bytes);
  if (entityKind === g.YAS_GIT_ENTITY_REF) return decodeGitRefEntityBody(bytes);
  if (entityKind === g.YAS_GIT_ENTITY_REMOTE)
    return decodeGitRemoteEntityBody(bytes);
  if (entityKind === g.YAS_GIT_ENTITY_OPERATION)
    return decodeGitOperationEntityBody(bytes);
  if (entityKind === g.YAS_GIT_ENTITY_STATUS)
    return decodeGitStatusEntityBody(bytes);
  if (entityKind === g.YAS_GIT_ENTITY_UPSTREAM)
    return decodeGitUpstreamEntityBody(bytes);
  if (entityKind === g.YAS_GIT_ENTITY_STASH)
    return decodeGitStashEntityBody(bytes);
  if (entityKind === g.YAS_GIT_ENTITY_WORKTREE_GENERATION)
    return decodeGitWorktreeGenerationEntityBody(bytes);
  throw new YasProtocolError("unknown Git entity kind");
}

export function encodeGitEntityRecord(value: YasGitEntityRecord): Uint8Array {
  validateEntity(value);
  return new YasWriter()
    .u16(value.entityKind)
    .u16(0)
    .bytesU16(value.key)
    .u64(value.revision)
    .bytesU32(encodeGitEntityBody(value.body))
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeGitEntityRecord(bytes: Uint8Array): YasGitEntityRecord {
  const cursor = new YasCursor(bytes);
  const entityKind = cursor.u16("Git entity kind");
  if (cursor.u16("Git entity reserved") !== 0)
    throw new YasProtocolError("Git entity reserved field is nonzero");
  const value = {
    entityKind,
    key: new Uint8Array(cursor.bytesU16("Git entity key")),
    revision: cursor.u64("Git entity revision"),
    body: decodeGitEntityBody(entityKind, cursor.bytesU32("Git entity body")),
    extensions: decodeExtensions(cursor, new Set(), "Git entity extensions"),
  };
  cursor.end("Git entity");
  validateEntity(value);
  return value;
}

export function encodeGitEntityPatch(value: YasGitEntityPatch): Uint8Array {
  if (
    value.fields === 0 ||
    value.fields & ~g.YAS_GIT_ENTITY_PATCH_FIELDS ||
    value.entityKind !== value.replacement.entityKind ||
    !equal(value.key, value.replacement.key) ||
    value.observedRevision >= value.replacement.revision
  )
    throw new YasProtocolError("invalid Git entity patch");
  validateEntity(value.replacement);
  rejectRequired(value.extensions, "Git entity patch");
  return new YasWriter()
    .u16(value.entityKind)
    .u16(0)
    .bytesU16(value.key)
    .u64(value.observedRevision)
    .u32(value.fields)
    .bytesU32(encodeGitEntityRecord(value.replacement))
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeGitEntityPatch(bytes: Uint8Array): YasGitEntityPatch {
  const cursor = new YasCursor(bytes);
  const entityKind = cursor.u16("Git entity kind");
  if (cursor.u16("Git patch reserved") !== 0)
    throw new YasProtocolError("Git patch reserved field is nonzero");
  const value = {
    entityKind,
    key: new Uint8Array(cursor.bytesU16("Git entity key")),
    observedRevision: cursor.u64("Git observed revision"),
    fields: cursor.u32("Git patch fields"),
    replacement: decodeGitEntityRecord(cursor.bytesU32("Git replacement")),
    extensions: decodeExtensions(cursor, new Set(), "Git patch extensions"),
  };
  cursor.end("Git entity patch");
  encodeGitEntityPatch(value);
  return value;
}

export function encodeGitRemovedEntity(value: YasGitRemovedEntity): Uint8Array {
  validateEntityIdentity(value.entityKind, value.key, value.revision);
  return new YasWriter()
    .u16(value.entityKind)
    .u16(0)
    .bytesU16(value.key)
    .u64(value.revision)
    .finish();
}

export function decodeGitRemovedEntity(bytes: Uint8Array): YasGitRemovedEntity {
  const cursor = new YasCursor(bytes);
  const entityKind = cursor.u16("Git entity kind");
  if (cursor.u16("Git remove reserved") !== 0)
    throw new YasProtocolError("Git remove reserved field is nonzero");
  const value = {
    entityKind,
    key: new Uint8Array(cursor.bytesU16("Git entity key")),
    revision: cursor.u64("Git removed revision"),
  };
  cursor.end("Git removed entity");
  encodeGitRemovedEntity(value);
  return value;
}

export class YasGitCatalog {
  private current = new Map<string, YasGitEntityRecord>();
  private currentRetention: YasStateCatalogueRetention<string>;
  private staging: Map<string, YasGitEntityRecord> | null = null;
  private stagingRetention: YasStateCatalogueRetention<string> | null = null;
  private subscription: YasStateSubscription | null = null;
  private revision = 0n;
  private listeners = new Set<(snapshot: YasGitSnapshot) => void>();
  private snapshotRejectors = new Set<(error: Error) => void>();
  private removeInvalidation: (() => void) | null;
  private watchPromise: Promise<void> | null = null;
  private cancelPendingWatch: ((error: Error) => void) | null = null;
  private generation = 0;
  private disposed = false;

  constructor(
    private readonly connection: YasConnection,
    readonly repositoryHandle: bigint,
  ) {
    this.currentRetention =
      YasStateCatalogueRetention.forConnection(connection);
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family === undefined || family === g.YAS_FAMILY_GIT)
        this.invalidateLocal();
    });
  }

  get snapshot(): YasGitSnapshot {
    return { revision: this.revision, entities: [...this.current.values()] };
  }

  subscribe(listener: (snapshot: YasGitSnapshot) => void): () => void {
    this.assertOpen();
    this.listeners.add(listener);
    invokeLifecycleListener(listener, this.snapshot, "Git catalogue");
    return () => this.listeners.delete(listener);
  }

  async firstSnapshot(
    options: YasWatchOptions & YasGitWatchOptions & { datasets?: number } = {},
  ): Promise<YasGitSnapshot> {
    this.assertOpen();
    if (this.revision !== 0n && this.subscription?.active) return this.snapshot;
    let remove: (() => void) | undefined;
    let rejectLifecycle!: (error: Error) => void;
    const result = new Promise<YasGitSnapshot>((resolve) => {
      remove = this.subscribe((snapshot) => {
        if (snapshot.revision === 0n) return;
        remove?.();
        resolve(snapshot);
      });
    });
    const cancelled = new Promise<never>((_resolve, reject) => {
      rejectLifecycle = reject;
      this.snapshotRejectors.add(reject);
    });
    try {
      return await Promise.race([
        this.watch(options).then(() => result),
        cancelled,
      ]);
    } finally {
      remove?.();
      this.snapshotRejectors.delete(rejectLifecycle);
    }
  }

  async watch(
    options: YasWatchOptions & YasGitWatchOptions & { datasets?: number } = {},
  ): Promise<void> {
    this.assertOpen();
    if (this.subscription?.active) return;
    if (this.watchPromise) return this.watchPromise;
    this.clearState();
    const generation = this.generation;
    const operation = YasStateSubscription.watch(
      this.connection,
      g.YAS_FAMILY_GIT,
      g.YAS_GIT_WATCH,
      g.YAS_GIT_UNWATCH,
      g.YAS_GIT_STATE,
      g.YAS_GIT_STATE_ACK,
      {
        ...options,
        extensions: [
          ...(options.extensions ?? []),
          ...encodeGitWatchOptions(options),
        ],
      },
      (batch) => this.apply(batch),
      {
        knownRecordKinds: new Set([
          YAS_STATE_ADD,
          YAS_STATE_REPLACE,
          YAS_STATE_PATCH,
          YAS_STATE_REMOVE,
        ]),
      },
      (statePayload) =>
        encodeGitWatch(
          this.repositoryHandle,
          options.datasets ?? g.YAS_GIT_WATCH_DATASETS,
          statePayload,
        ),
    ).then(async (subscription) => {
      if (this.disposed || generation !== this.generation) {
        await subscription.unwatch().catch(() => undefined);
        throw new YasProtocolError(
          "Git catalogue changed while WATCH was pending",
        );
      }
      this.subscription = subscription;
    });
    let cancel!: (error: Error) => void;
    const cancelled = new Promise<never>((_resolve, reject) => {
      cancel = reject;
    });
    this.cancelPendingWatch = cancel;
    const pending = Promise.race([operation, cancelled]);
    this.watchPromise = pending;
    try {
      await pending;
    } finally {
      if (this.watchPromise === pending) this.watchPromise = null;
      if (this.cancelPendingWatch === cancel) this.cancelPendingWatch = null;
    }
  }

  async unwatch(): Promise<void> {
    this.cancelWatch("Git catalogue WATCH was cancelled");
    this.cancelSnapshots("Git catalogue snapshot wait was cancelled");
    this.generation++;
    const subscription = this.subscription;
    this.subscription = null;
    this.clearState();
    await subscription?.unwatch();
  }

  async dispose(): Promise<void> {
    if (this.disposed) return;
    this.disposed = true;
    this.cancelWatch("Git catalogue was closed while WATCH was pending");
    this.cancelSnapshots("Git catalogue closed before its first snapshot");
    this.generation++;
    this.removeInvalidation?.();
    this.removeInvalidation = null;
    const subscription = this.subscription;
    this.subscription = null;
    this.clearState();
    this.listeners.clear();
    await subscription?.unwatch();
  }

  private apply(batch: YasStateBatch): void {
    if (this.disposed) return;
    if (batch.phase === YAS_STATE_RESET) {
      this.currentRetention.dispose();
      this.stagingRetention?.dispose();
      this.current = new Map();
      this.currentRetention = YasStateCatalogueRetention.forConnection(
        this.connection,
      );
      this.staging = null;
      this.stagingRetention = null;
      this.revision = 0n;
      this.emit();
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_BEGIN) {
      this.stagingRetention?.dispose();
      this.staging = new Map();
      this.stagingRetention = YasStateCatalogueRetention.forConnection(
        this.connection,
      );
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_RECORDS) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("Git snapshot records without begin");
      try {
        this.applyRecords(this.staging, this.stagingRetention, batch.records);
      } catch (error) {
        this.discardStaging();
        throw error;
      }
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_END) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("Git snapshot end without begin");
      try {
        this.applyRecords(this.staging, this.stagingRetention, batch.records);
      } catch (error) {
        this.discardStaging();
        throw error;
      }
      const previousRetention = this.currentRetention;
      this.current = this.staging;
      this.currentRetention = this.stagingRetention;
      this.staging = null;
      this.stagingRetention = null;
      previousRetention.dispose();
      this.revision = batch.toRevision;
      this.emit();
      return;
    }
    if (batch.phase === YAS_STATE_DELTA) {
      const nextRetention = this.currentRetention.clone();
      let next: Map<string, YasGitEntityRecord>;
      try {
        next = new Map(this.current);
        this.applyRecords(next, nextRetention, batch.records);
      } catch (error) {
        nextRetention.dispose();
        throw error;
      }
      const previousRetention = this.currentRetention;
      this.current = next;
      this.currentRetention = nextRetention;
      previousRetention.dispose();
      this.revision = batch.toRevision;
      this.emit();
    }
  }

  private applyRecords(
    target: Map<string, YasGitEntityRecord>,
    retention: YasStateCatalogueRetention<string>,
    records: readonly YasTypedRecord[],
  ): void {
    const originals = new Map<string, YasGitEntityRecord | null>();
    const remember = (key: string) => {
      if (!originals.has(key)) originals.set(key, target.get(key) ?? null);
    };
    const replace = (key: string, decoded: YasGitEntityRecord) => {
      const encoded = encodeGitEntityRecord(decoded);
      const entity = decodeGitEntityRecord(encoded);
      remember(key);
      retention.upsert(
        key,
        Math.max(encoded.length, estimateStateRetainedBytes(entity)),
      );
      target.set(key, entity);
    };
    try {
      for (const record of records) {
        if (
          record.kind === YAS_STATE_ADD ||
          record.kind === YAS_STATE_REPLACE
        ) {
          const entity = decodeGitEntityRecord(record.body);
          const key = entityKey(entity.entityKind, entity.key);
          const exists = target.has(key);
          if ((record.kind === YAS_STATE_ADD) === exists)
            throw new YasProtocolError("Git ADD/REPLACE precondition failed");
          replace(key, entity);
        } else if (record.kind === YAS_STATE_PATCH) {
          const patch = decodeGitEntityPatch(record.body);
          const key = entityKey(patch.entityKind, patch.key);
          const previous = target.get(key);
          if (!previous || previous.revision !== patch.observedRevision)
            throw new YasProtocolError("Git PATCH precondition failed");
          replace(key, patch.replacement);
        } else if (record.kind === YAS_STATE_REMOVE) {
          const removed = decodeGitRemovedEntity(record.body);
          const key = entityKey(removed.entityKind, removed.key);
          const previous = target.get(key);
          if (!previous)
            throw new YasProtocolError("Git REMOVE names unknown entity");
          remember(key);
          retention.remove(key);
          target.delete(key);
        } else throw new YasProtocolError("unsupported Git state record kind");
      }
    } catch (error) {
      for (const key of originals.keys()) retention.remove(key);
      for (const [key, original] of originals) {
        if (original) {
          retention.upsert(
            key,
            Math.max(
              encodeGitEntityRecord(original).length,
              estimateStateRetainedBytes(original),
            ),
          );
          target.set(key, original);
        } else target.delete(key);
      }
      throw error;
    }
  }

  private invalidateLocal(): void {
    if (this.disposed) return;
    this.cancelWatch("Git catalogue was invalidated while WATCH was pending");
    this.cancelSnapshots("Git catalogue invalidated before its first snapshot");
    this.generation++;
    this.subscription = null;
    this.clearState();
  }

  private clearState(): void {
    this.currentRetention.dispose();
    this.stagingRetention?.dispose();
    this.current = new Map();
    this.currentRetention = YasStateCatalogueRetention.forConnection(
      this.connection,
    );
    this.staging = null;
    this.stagingRetention = null;
    this.revision = 0n;
    this.emit();
  }

  private discardStaging(): void {
    this.stagingRetention?.dispose();
    this.staging = null;
    this.stagingRetention = null;
  }

  private emit(): void {
    if (this.disposed) return;
    const snapshot = this.snapshot;
    for (const listener of this.listeners)
      invokeLifecycleListener(listener, snapshot, "Git catalogue");
  }

  private assertOpen(): void {
    if (this.disposed) throw new YasProtocolError("Git catalogue is closed");
  }

  private cancelWatch(message: string): void {
    const cancel = this.cancelPendingWatch;
    this.cancelPendingWatch = null;
    cancel?.(new YasProtocolError(message));
  }

  private cancelSnapshots(message: string): void {
    const error = new YasProtocolError(message);
    for (const reject of this.snapshotRejectors) reject(error);
    this.snapshotRejectors.clear();
  }
}

export class YasGitClient {
  private readonly transfers;
  private readonly repositories = new Map<bigint, YasGitRepository>();
  private readonly progressListeners = new Set<
    (progress: YasGitProgress) => void
  >();
  private readonly removeEvents: Array<() => void> = [];
  private removeInvalidation: (() => void) | null;
  private generation = 0;
  private disposed = false;

  constructor(readonly connection: YasConnection) {
    connection.family(g.YAS_FAMILY_GIT, g.YAS_GIT_VERSION);
    this.transfers = transfersFor(connection);
    this.removeEvents.push(
      connection.onEvent(
        g.YAS_FAMILY_GIT,
        g.YAS_GIT_PROGRESS,
        ({ payload }) => {
          const progress = decodeGitProgress(payload);
          for (const listener of this.progressListeners)
            invokeLifecycleListener(listener, progress, "Git progress");
        },
      ),
      connection.onEvent(g.YAS_FAMILY_GIT, g.YAS_GIT_CLOSED, ({ payload }) => {
        const event = decodeGitClosed(payload);
        this.repositories.get(event.repositoryHandle)?.markClosed(event);
      }),
    );
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family !== undefined && family !== g.YAS_FAMILY_GIT) return;
      this.generation++;
      for (const repository of [...this.repositories.values()])
        repository.invalidate();
      this.repositories.clear();
    });
  }

  onProgress(listener: (progress: YasGitProgress) => void): () => void {
    this.assertOpen();
    this.progressListeners.add(listener);
    return () => this.progressListeners.delete(listener);
  }

  async open(value: YasGitOpen): Promise<YasGitRepository> {
    this.assertOpen();
    const generation = this.generation;
    const opened = await this.connection.requestDecoded(
      g.YAS_FAMILY_GIT,
      g.YAS_GIT_OPEN,
      encodeGitOpen(value),
      decodeGitOpenResult,
    );
    const repository = new YasGitRepository(this, opened);
    if (this.disposed || generation !== this.generation) {
      await repository.close().catch(() => undefined);
      throw new YasProtocolError("Git client changed while OPEN was pending");
    }
    this.repositories.set(opened.repositoryHandle, repository);
    return repository;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.generation++;
    for (const remove of this.removeEvents.splice(0)) remove();
    this.removeInvalidation?.();
    this.removeInvalidation = null;
    this.progressListeners.clear();
    for (const repository of [...this.repositories.values()])
      void repository.close().catch(() => undefined);
    this.repositories.clear();
  }

  async discover(
    source: YasGitRepositorySource,
    options: {
      maxDepth?: number;
      flags?: number;
      maxRecords?: number;
      cursor?: YasGitQueryCursor;
      initialReceiveCredit?: bigint;
    } = {},
  ): Promise<YasGitQueryPage> {
    this.assertOpen();
    const lease = this.transfers.reserveReceiveCredit(
      options.initialReceiveCredit ?? 1024n * 1024n,
      1n,
    );
    let accepted = false;
    let released = false;
    try {
      return await this.connection.requestDecoded(
        g.YAS_FAMILY_GIT,
        g.YAS_GIT_QUERY,
        encodeGitQuery({
          repositoryHandle: 0n,
          maxRecords: options.maxRecords ?? 256,
          cursor: options.cursor ?? { kind: "start" },
          initialReceiveCredit: lease.bytes,
          body: {
            kind: "discover",
            source,
            maxDepth: options.maxDepth ?? 0,
            flags: options.flags ?? 0,
          },
        }),
        (payload) => {
          const page = decodeGitQueryPage(payload);
          if (page.delivery.kind === "inline") {
            lease.release();
            released = true;
            const records = page.delivery.records.map(cloneQueryRecord);
            return {
              nextCursor: cloneQueryCursor(page.nextCursor),
              totalHint: page.totalHint,
              flags: page.flags,
              records: () => Promise.resolve(records.map(cloneQueryRecord)),
            };
          }
          const transfer = this.transfers.acceptServerDescriptor(
            page.delivery.descriptor,
            lease,
          );
          accepted = true;
          return {
            nextCursor: cloneQueryCursor(page.nextCursor),
            totalHint: page.totalHint,
            flags: page.flags,
            records: () => collectGitQueryRecords(transfer),
          };
        },
      );
    } catch (error) {
      if (!accepted && !released) lease.release();
      throw error;
    }
  }

  transferManager() {
    return this.transfers;
  }

  release(repository: YasGitRepository): void {
    if (this.repositories.get(repository.handle) === repository)
      this.repositories.delete(repository.handle);
  }

  private assertOpen(): void {
    if (this.disposed) throw new YasProtocolError("Git client is closed");
  }
}

export class YasGitRepository {
  readonly catalog: YasGitCatalog;
  private closed = false;
  private closeEvent: YasGitClosed | null = null;
  private readonly closeListeners = new Set<(event: YasGitClosed) => void>();
  private readonly watchedQueries = new Set<YasGitWatchedQuery>();
  private readonly watchedQueryCancellations = new Set<
    (error: Error) => void
  >();

  constructor(
    readonly client: YasGitClient,
    readonly opened: YasGitOpenResult,
  ) {
    this.catalog = new YasGitCatalog(
      client.connection,
      opened.repositoryHandle,
    );
  }

  get handle(): bigint {
    return this.opened.repositoryHandle;
  }

  list(
    options: YasWatchOptions & YasGitWatchOptions & { datasets?: number } = {},
  ): Promise<YasGitSnapshot> {
    this.assertOpen();
    return this.catalog.firstSnapshot(options);
  }

  async close(extensions: readonly YasExtension[] = []): Promise<void> {
    if (this.closed) return;
    const watchedQueries = [...this.watchedQueries];
    const event = {
      repositoryHandle: this.handle,
      repositoryRevision: this.opened.repositoryRevision,
      reason: g.YAS_GIT_CLOSED_CLIENT_REQUEST,
      detail: "",
    };
    this.finish(event, false);
    await Promise.allSettled(watchedQueries.map((query) => query.close()));
    await this.catalog.dispose().catch(() => undefined);
    await this.client.connection.request(
      g.YAS_FAMILY_GIT,
      g.YAS_GIT_CLOSE,
      encodeGitClose(this.handle, extensions),
    );
  }

  onClosed(listener: (event: YasGitClosed) => void): () => void {
    this.closeListeners.add(listener);
    if (this.closeEvent)
      invokeLifecycleListener(listener, this.closeEvent, "Git close");
    return () => this.closeListeners.delete(listener);
  }

  markClosed(event: YasGitClosed): void {
    this.finish(event);
  }

  invalidate(): void {
    this.markClosed({
      repositoryHandle: this.handle,
      repositoryRevision: this.opened.repositoryRevision,
      reason: g.YAS_GIT_CLOSED_BACKEND_FAILED,
      detail: "Git session invalidated",
    });
  }

  async query(
    body: YasGitQueryBody,
    options: {
      maxRecords?: number;
      cursor?: YasGitQueryCursor;
      initialReceiveCredit?: bigint;
      extensions?: readonly YasExtension[];
    } = {},
  ): Promise<YasGitQueryPage> {
    this.assertOpen();
    const manager = this.client.transferManager();
    const lease = manager.reserveReceiveCredit(
      options.initialReceiveCredit ?? 1024n * 1024n,
      1n,
    );
    let accepted = false;
    let released = false;
    try {
      return await this.client.connection.requestDecoded(
        g.YAS_FAMILY_GIT,
        g.YAS_GIT_QUERY,
        encodeGitQuery({
          repositoryHandle: this.handle,
          maxRecords: options.maxRecords ?? 256,
          cursor: options.cursor ?? { kind: "start" },
          initialReceiveCredit: lease.bytes,
          body,
          extensions: options.extensions,
        }),
        (payload) => {
          const page = decodeGitQueryPage(payload);
          if (page.delivery.kind === "inline") {
            lease.release();
            released = true;
            const records = page.delivery.records.map(cloneQueryRecord);
            return {
              nextCursor: cloneQueryCursor(page.nextCursor),
              totalHint: page.totalHint,
              flags: page.flags,
              records: () => Promise.resolve(records.map(cloneQueryRecord)),
            };
          }
          const transfer = manager.acceptServerDescriptor(
            page.delivery.descriptor,
            lease,
          );
          accepted = true;
          return {
            nextCursor: cloneQueryCursor(page.nextCursor),
            totalHint: page.totalHint,
            flags: page.flags,
            records: () => collectGitQueryRecords(transfer),
          };
        },
      );
    } catch (error) {
      if (!accepted && !released) lease.release();
      throw error;
    }
  }

  fetch(
    value: Omit<YasGitFetch, "repositoryHandle">,
  ): Promise<YasGitFetchResult> {
    this.assertOpen();
    return this.client.connection.requestDecoded(
      g.YAS_FAMILY_GIT,
      g.YAS_GIT_FETCH,
      encodeGitFetch({ ...value, repositoryHandle: this.handle }),
      decodeGitFetchResult,
    );
  }

  async watchQuery(
    body: YasGitQueryBody,
    onUpdate: (update: YasGitWatchedQueryUpdate) => void,
    options: YasWatchOptions & { maxRecords?: number } = {},
  ): Promise<YasGitWatchedQuery> {
    this.assertOpen();
    const operation = YasGitWatchedQuery.open(
      this,
      body,
      onUpdate,
      options,
    ).then(async (query) => {
      if (this.closed) {
        await query.close().catch(() => undefined);
        throw new YasProtocolError(
          "Git repository closed while WATCH QUERY was pending",
        );
      }
      this.watchedQueries.add(query);
      return query;
    });
    let cancel!: (error: Error) => void;
    const cancelled = new Promise<never>((_resolve, reject) => {
      cancel = reject;
      this.watchedQueryCancellations.add(reject);
    });
    try {
      return await Promise.race([operation, cancelled]);
    } finally {
      this.watchedQueryCancellations.delete(cancel);
    }
  }

  releaseWatchedQuery(query: YasGitWatchedQuery): void {
    this.watchedQueries.delete(query);
  }

  async content(
    record: YasGitContentRecord,
    initialCredit = 1024n * 1024n,
  ): Promise<Uint8Array> {
    if (record.delivery.kind === "inline")
      return new Uint8Array(record.delivery.bytes);
    const manager = this.client.transferManager();
    const lease = manager.reserveReceiveCredit(initialCredit, 1n);
    let accepted = false;
    try {
      const transfer = manager.acceptServerDescriptor(
        record.delivery.descriptor,
        lease,
      );
      accepted = true;
      const chunks: Uint8Array[] = [];
      let length = 0;
      while (true) {
        const chunk = await transfer.read();
        if (chunk === null) break;
        length += chunk.length;
        if (BigInt(length) > record.byteLength)
          throw new YasProtocolError("Git content exceeds declared length");
        chunks.push(chunk);
      }
      if (BigInt(length) !== record.nextOffset - record.offset)
        throw new YasProtocolError("Git content length does not match window");
      const output = new Uint8Array(length);
      let offset = 0;
      for (const chunk of chunks) {
        output.set(chunk, offset);
        offset += chunk.length;
      }
      return output;
    } catch (error) {
      if (!accepted) lease.release();
      throw error;
    }
  }

  private assertOpen(): void {
    if (this.closed) throw new YasProtocolError("Git repository is closed");
  }

  private finish(event: YasGitClosed, disposeResources = true): void {
    if (this.closeEvent) return;
    this.closed = true;
    this.closeEvent = event;
    const cancellationError = new YasProtocolError(
      "Git repository closed while WATCH QUERY was pending",
    );
    for (const cancel of this.watchedQueryCancellations)
      cancel(cancellationError);
    this.watchedQueryCancellations.clear();
    this.client.release(this);
    if (disposeResources) {
      void this.catalog.dispose().catch(() => undefined);
      for (const query of [...this.watchedQueries])
        void query.close().catch(() => undefined);
    }
    this.watchedQueries.clear();
    for (const listener of this.closeListeners)
      invokeLifecycleListener(listener, event, "Git close");
    this.closeListeners.clear();
  }
}

/** A watched Git query using nested common State framing. */
export class YasGitWatchedQuery {
  private closed = false;
  private appliedRevision = 0n;
  private snapshotTarget: bigint | null = null;
  private pendingUpdate: YasGitWatchedQueryUpdate | null = null;
  private cumulativeCredit: bigint;
  private removeEvent: (() => void) | null = null;
  private removeInvalidation: (() => void) | null = null;
  private leaseReleased = false;

  private constructor(
    readonly repository: YasGitRepository,
    readonly subscriptionId: number,
    private readonly lease: YasReceiveBudgetLease,
    private readonly onUpdate: (update: YasGitWatchedQueryUpdate) => void,
  ) {
    this.cumulativeCredit = lease.bytes;
    this.removeEvent = repository.client.connection.onEvent(
      g.YAS_FAMILY_GIT,
      g.YAS_GIT_QUERY_STATE,
      ({ payload }) => this.handle(payload),
    );
    this.removeInvalidation = repository.client.connection.onInvalidation(
      ({ family }) => {
        if (family === undefined || family === g.YAS_FAMILY_GIT)
          this.closeLocal();
      },
    );
  }

  static async open(
    repository: YasGitRepository,
    body: YasGitQueryBody,
    onUpdate: (update: YasGitWatchedQueryUpdate) => void,
    options: YasWatchOptions & { maxRecords?: number },
  ): Promise<YasGitWatchedQuery> {
    const preferred = options.initialCredit ?? 1024n * 1024n;
    const lease = repository.client.connection.receiveBudget.reserve(
      preferred,
      1024n,
    );
    try {
      return await repository.client.connection.requestDecoded(
        g.YAS_FAMILY_GIT,
        g.YAS_GIT_WATCH_QUERY,
        encodeGitWatchQuery(
          repository.handle,
          options.maxRecords ?? 0,
          body,
          encodeWatch(options, lease.bytes),
        ),
        (payload) => {
          const result = decodeWatchResult(payload);
          return new YasGitWatchedQuery(
            repository,
            result.subscriptionId,
            lease,
            onUpdate,
          );
        },
      );
    } catch (error) {
      lease.release();
      throw error;
    }
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    try {
      await this.repository.client.connection.request(
        g.YAS_FAMILY_GIT,
        g.YAS_GIT_UNWATCH_QUERY,
        encodeGitUnwatchQuery(this.subscriptionId),
      );
    } finally {
      this.closeLocal();
    }
  }

  private handle(payload: Uint8Array): void {
    if (this.closed) return;
    const decoded = decodeGitQueryState(payload);
    if (decoded.querySubscriptionId !== this.subscriptionId) return;
    const { batch } = decoded.event;
    this.validateSequence(batch);
    if (batch.phase === YAS_STATE_SNAPSHOT_RECORDS) {
      if (this.pendingUpdate)
        throw new YasProtocolError(
          "Git watched query has duplicate snapshot page",
        );
      this.pendingUpdate = this.decodeUpdate(batch, YAS_STATE_ADD);
    } else if (batch.phase === YAS_STATE_SNAPSHOT_END) {
      if (batch.records.length !== 0)
        throw new YasProtocolError("Git watched query marker contains records");
      this.appliedRevision = batch.toRevision;
      if (!this.pendingUpdate)
        throw new YasProtocolError("Git watched query snapshot has no page");
      const update = this.pendingUpdate;
      this.pendingUpdate = null;
      invokeLifecycleListener(this.onUpdate, update, "Git watched query");
    } else if (batch.phase === YAS_STATE_DELTA) {
      this.appliedRevision = batch.toRevision;
      invokeLifecycleListener(
        this.onUpdate,
        this.decodeUpdate(batch, YAS_STATE_REPLACE),
        "Git watched query",
      );
    } else if (batch.phase === YAS_STATE_RESET) {
      this.pendingUpdate = null;
    }
    this.cumulativeCredit += BigInt(payload.length);
    this.repository.client.connection.sendEvent(
      g.YAS_FAMILY_GIT,
      g.YAS_GIT_QUERY_STATE_ACK,
      new YasWriter()
        .u32(this.subscriptionId)
        .u64(this.appliedRevision)
        .u64(this.cumulativeCredit)
        .finish(),
    );
  }

  private decodeUpdate(
    batch: YasStateBatch,
    expectedKind: number,
  ): YasGitWatchedQueryUpdate {
    if (batch.records.length !== 1 || batch.records[0].kind !== expectedKind)
      throw new YasProtocolError("invalid Git watched query state record");
    const value = decodeGitWatchedQueryValue(batch.records[0].body);
    if (!value.page) return { status: value.status, detail: value.detail };
    const records =
      value.page.delivery.kind === "inline"
        ? value.page.delivery.records.map(cloneQueryRecord)
        : [];
    return {
      status: value.status,
      detail: value.detail,
      page: {
        nextCursor: cloneQueryCursor(value.page.nextCursor),
        totalHint: value.page.totalHint,
        flags: value.page.flags,
        records: () => Promise.resolve(records.map(cloneQueryRecord)),
      },
    };
  }

  private validateSequence(batch: YasStateBatch): void {
    if (batch.phase === YAS_STATE_SNAPSHOT_BEGIN) {
      if (batch.fromRevision !== 0n || this.snapshotTarget !== null)
        throw new YasProtocolError("invalid Git watched snapshot begin");
      this.snapshotTarget = batch.toRevision;
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_RECORDS) {
      if (
        this.snapshotTarget === null ||
        batch.fromRevision !== this.snapshotTarget ||
        batch.toRevision !== this.snapshotTarget
      )
        throw new YasProtocolError("invalid Git watched snapshot records");
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_END) {
      if (
        this.snapshotTarget === null ||
        batch.fromRevision !== this.snapshotTarget ||
        batch.toRevision !== this.snapshotTarget
      )
        throw new YasProtocolError("invalid Git watched snapshot end");
      this.snapshotTarget = null;
      return;
    }
    if (batch.phase === YAS_STATE_DELTA) {
      if (
        this.snapshotTarget !== null ||
        batch.fromRevision !== this.appliedRevision ||
        batch.toRevision <= batch.fromRevision
      )
        throw new YasProtocolError("Git watched query state has a gap");
      return;
    }
    if (batch.fromRevision !== this.appliedRevision)
      throw new YasProtocolError("invalid Git watched query reset");
    this.snapshotTarget = null;
  }

  private closeLocal(): void {
    if (!this.closed) this.closed = true;
    this.repository.releaseWatchedQuery(this);
    this.removeEvent?.();
    this.removeEvent = null;
    this.removeInvalidation?.();
    this.removeInvalidation = null;
    if (!this.leaseReleased) {
      this.leaseReleased = true;
      this.lease.release();
    }
  }
}

function invokeLifecycleListener<T>(
  listener: (value: T) => void,
  value: T,
  kind: string,
): void {
  try {
    listener(value);
  } catch (error) {
    reportLifecycleListenerError(kind, error);
  }
}

function reportLifecycleListenerError(kind: string, error: unknown): void {
  try {
    const report = (
      globalThis as typeof globalThis & {
        reportError?: (value: unknown) => void;
      }
    ).reportError;
    if (report) report(error);
    else console.error(`YAS ${kind} listener failed`, error);
  } catch {
    // Resource cleanup must not depend on host error reporting.
  }
}

async function collectGitQueryRecords(
  transfer: YasTransfer,
): Promise<readonly YasGitQueryRecord[]> {
  const records: YasGitQueryRecord[] = [];
  let bytes = 0;
  try {
    while (true) {
      const message = await transfer.readMessage();
      if (message === null) break;
      bytes += message.length;
      if (bytes > g.YAS_GIT_MAX_QUERY_BYTES)
        throw new YasProtocolError("Git query Transfer exceeds byte limit");
      const cursor = new YasCursor(message);
      while (cursor.remaining !== 0) {
        const record = decodeGitQueryRecord(cursor);
        if (record) records.push(record);
        if (records.length > g.YAS_GIT_MAX_QUERY_RECORDS)
          throw new YasProtocolError("Git query Transfer exceeds record limit");
      }
    }
    return records;
  } catch (error) {
    transfer.reset();
    throw error;
  }
}

function decodeObjectId(cursor: YasCursor): YasGitObjectId {
  const algorithm = cursor.u8("Git object algorithm");
  const length = cursor.u8("Git object ID length");
  if (cursor.u16("Git object ID reserved") !== 0)
    throw new YasProtocolError("Git object ID reserved field is nonzero");
  const value = {
    algorithm,
    bytes: new Uint8Array(cursor.take(length, "Git object ID")),
  };
  validateObjectId(value);
  return value;
}

function validateObjectId(value: YasGitObjectId): void {
  const expected =
    value.algorithm === g.YAS_GIT_OBJECT_SHA1
      ? 20
      : value.algorithm === g.YAS_GIT_OBJECT_SHA256
        ? 32
        : 0;
  if (value.bytes.length !== expected)
    throw new YasProtocolError("invalid Git object ID");
}

function validateOpenResult(value: YasGitOpenResult): void {
  requireHandle(value.repositoryHandle, "Git repository handle");
  requireRevision(value.repositoryRevision, "Git repository revision");
  const bare = Boolean(value.repositoryFlags & g.YAS_GIT_REPOSITORY_BARE);
  if (
    value.objectAlgorithm > g.YAS_GIT_OBJECT_SHA256 ||
    value.repositoryFlags & ~g.YAS_GIT_REPOSITORY_FLAGS ||
    bare !== (value.canonicalWorktreePath.length === 0) ||
    value.canonicalWorktreePath.length > g.YAS_GIT_MAX_PATH_BYTES ||
    value.canonicalWorktreePath.includes(0) ||
    value.canonicalGitDir.length === 0 ||
    value.canonicalGitDir.length > g.YAS_GIT_MAX_PATH_BYTES ||
    value.canonicalGitDir.includes(0)
  )
    throw new YasProtocolError("invalid Git OPEN Result metadata");
  rejectRequired(value.extensions, "Git OPEN Result");
}

function validatePlatformPath(value: Uint8Array, field: string): void {
  if (
    value.length === 0 ||
    value.length > g.YAS_GIT_MAX_PATH_BYTES ||
    value.includes(0)
  )
    throw new YasProtocolError(`invalid ${field}`);
}

function validateSpec(value: Uint8Array): void {
  if (
    value.length === 0 ||
    value.length > g.YAS_GIT_MAX_SPEC_BYTES ||
    value.includes(0)
  )
    throw new YasProtocolError("invalid Git revision specification");
}

function validateRaw(
  value: Uint8Array,
  maximum: number,
  forbidNul: boolean,
  field: string,
): void {
  if (
    value.length === 0 ||
    value.length > maximum ||
    (forbidNul && value.includes(0))
  )
    throw new YasProtocolError(`invalid ${field}`);
}

function validateOptionalRaw(
  value: Uint8Array,
  maximum: number,
  forbidNul: boolean,
  field: string,
): void {
  if (value.length > maximum || (forbidNul && value.includes(0)))
    throw new YasProtocolError(`invalid ${field}`);
}

function validateQueryCursorForBody(
  cursor: YasGitQueryCursor,
  body: YasGitQueryBody,
): void {
  if (cursor.kind === "start") return;
  const valid =
    (body.kind === "log" && cursor.kind === "log-frontier") ||
    (["tree", "diff", "index"].includes(body.kind) && cursor.kind === "path") ||
    (body.kind === "patch" && cursor.kind === "patch") ||
    (body.kind === "discover" && cursor.kind === "platform-path") ||
    (["blame", "reflog", "worktrees"].includes(body.kind) &&
      cursor.kind === "position");
  if (!valid) throw new YasProtocolError("invalid Git query cursor kind");
}

function validatePatchPaths(
  status: number,
  oldPath: YasFsPath | undefined,
  newPath: YasFsPath | undefined,
): void {
  const valid =
    status === g.YAS_GIT_DIFF_ADDED
      ? !oldPath && Boolean(newPath)
      : status === g.YAS_GIT_DIFF_DELETED
        ? Boolean(oldPath) && !newPath
        : (status === g.YAS_GIT_DIFF_MODIFIED ||
            status === g.YAS_GIT_DIFF_RENAMED ||
            status === g.YAS_GIT_DIFF_COPIED) &&
          Boolean(oldPath) &&
          Boolean(newPath);
  if (!valid) throw new YasProtocolError("invalid Git patch file paths");
  if (oldPath) requireNonRootPath(oldPath);
  if (newPath) requireNonRootPath(newPath);
}

function validatePatchRow(value: YasGitPatchRowRecord): void {
  requireU32(value.oldLine, "Git patch old line");
  requireU32(value.newLine, "Git patch new line");
  if (
    (value.oldLine === 0 && value.newLine === 0) ||
    value.oldText.length + value.newText.length > g.YAS_GIT_MAX_QUERY_BYTES ||
    (value.oldLine === 0 &&
      (value.oldText.length !== 0 || value.oldSpans.length !== 0)) ||
    (value.newLine === 0 &&
      (value.newText.length !== 0 || value.newSpans.length !== 0))
  )
    throw new YasProtocolError("invalid Git patch row");
  validatePatchSpans(value.oldSpans, value.oldText.length);
  validatePatchSpans(value.newSpans, value.newText.length);
}

function validatePatchSpans(
  spans: readonly YasGitPatchSpan[],
  textLength: number,
): void {
  if (spans.length > g.YAS_GIT_MAX_PATCH_SPANS)
    throw new YasProtocolError("too many Git patch spans");
  let previousEnd = 0;
  for (const span of spans) {
    requireU32(span.start, "Git patch span start");
    requireU32(span.length, "Git patch span length");
    const end = span.start + span.length;
    if (
      span.length === 0 ||
      span.start < previousEnd ||
      !Number.isSafeInteger(end) ||
      end > textLength
    )
      throw new YasProtocolError("invalid Git patch span");
    previousEnd = end;
  }
}

function encodePatchSpans(
  writer: YasWriter,
  spans: readonly YasGitPatchSpan[],
): void {
  writer.u16(spans.length).u16(0);
  for (const span of spans) writer.u32(span.start).u32(span.length);
}

function decodePatchSpans(cursor: YasCursor, side: string): YasGitPatchSpan[] {
  const count = cursor.u16(`Git ${side} patch span count`);
  if (
    cursor.u16(`Git ${side} patch spans reserved`) !== 0 ||
    count > g.YAS_GIT_MAX_PATCH_SPANS ||
    count > Math.floor(cursor.remaining / 8)
  )
    throw new YasProtocolError("invalid Git patch span count");
  const spans: YasGitPatchSpan[] = [];
  for (let index = 0; index < count; index++)
    spans.push({
      start: cursor.u32(`Git ${side} patch span start`),
      length: cursor.u32(`Git ${side} patch span length`),
    });
  return spans;
}

function requireU32(value: number, field: string): void {
  if (!Number.isInteger(value) || value < 0 || value > 0xffff_ffff)
    throw new YasProtocolError(`invalid ${field}`);
}

function cloneQueryCursor(value: YasGitQueryCursor): YasGitQueryCursor {
  return decodeGitQueryCursor(encodeGitQueryCursor(value));
}

function rejectUnknownRequired(
  extensions: readonly YasExtension[],
  known: ReadonlySet<number>,
  context: string,
): void {
  encodeExtensions(extensions);
  if (
    extensions.some(
      (extension) => extension.required && !known.has(extension.tag),
    )
  )
    throw new YasProtocolError(`unknown required ${context} extension`);
}

function compareBytes(left: Uint8Array, right: Uint8Array): number {
  const length = Math.min(left.length, right.length);
  for (let index = 0; index < length; index++) {
    const difference = left[index]! - right[index]!;
    if (difference !== 0) return difference;
  }
  return left.length - right.length;
}

function requireNonRootPath(value: YasFsPath): void {
  if (value.components.length === 0)
    throw new YasProtocolError("empty Git path");
  encodeFsPath(value);
}

function validatePageDescriptor(value: YasTransferDescriptor): void {
  if (
    value.mode !== YAS_TRANSFER_MODE_MESSAGE ||
    value.direction !== YAS_TRANSFER_SENDER_TO_RECEIVER ||
    value.contentFamily !== g.YAS_FAMILY_GIT ||
    value.contentKind !== g.YAS_GIT_QUERY_CONTENT_KIND ||
    value.contentVersion !== g.YAS_GIT_VERSION ||
    value.sensitiveContent !== true
  )
    throw new YasProtocolError("invalid Git query Transfer descriptor");
}

function validateContentDescriptor(
  value: YasTransferDescriptor,
  contentKind: number,
): void {
  if (
    value.mode !== YAS_TRANSFER_MODE_BYTE ||
    value.direction !== YAS_TRANSFER_SENDER_TO_RECEIVER ||
    value.maxItemBytes !== 0n ||
    value.contentFamily !== g.YAS_FAMILY_GIT ||
    value.contentKind !== contentKind ||
    value.contentVersion !== g.YAS_GIT_VERSION ||
    value.sensitiveContent !== true
  )
    throw new YasProtocolError("invalid Git content Transfer descriptor");
}

function validateEntity(value: YasGitEntityRecord): void {
  validateEntityIdentity(value.entityKind, value.key, value.revision);
  const expectedKind =
    value.body.kind === "head"
      ? g.YAS_GIT_ENTITY_HEAD
      : value.body.kind === "ref"
        ? g.YAS_GIT_ENTITY_REF
        : value.body.kind === "remote"
          ? g.YAS_GIT_ENTITY_REMOTE
          : value.body.kind === "operation"
            ? g.YAS_GIT_ENTITY_OPERATION
            : value.body.kind === "status"
              ? g.YAS_GIT_ENTITY_STATUS
              : value.body.kind === "upstream"
                ? g.YAS_GIT_ENTITY_UPSTREAM
                : value.body.kind === "stash"
                  ? g.YAS_GIT_ENTITY_STASH
                  : g.YAS_GIT_ENTITY_WORKTREE_GENERATION;
  if (value.entityKind !== expectedKind)
    throw new YasProtocolError("Git entity kind and body disagree");
  encodeGitEntityBody(value.body);
  rejectRequired(value.extensions, "Git entity");
}

function validateEntityIdentity(
  entityKind: number,
  key: Uint8Array,
  revision: bigint,
): void {
  if (
    entityKind > g.YAS_GIT_ENTITY_WORKTREE_GENERATION ||
    key.length > g.YAS_GIT_MAX_SPEC_BYTES
  )
    throw new YasProtocolError("invalid Git state entity");
  if (entityKind === g.YAS_GIT_ENTITY_HEAD) {
    if (!equal(key, new TextEncoder().encode("HEAD")))
      throw new YasProtocolError("invalid Git HEAD entity key");
  } else if (entityKind === g.YAS_GIT_ENTITY_OPERATION) {
    if (!equal(key, new TextEncoder().encode("operation")))
      throw new YasProtocolError("invalid Git operation entity key");
  } else if (
    entityKind === g.YAS_GIT_ENTITY_REF ||
    entityKind === g.YAS_GIT_ENTITY_REMOTE ||
    entityKind === g.YAS_GIT_ENTITY_UPSTREAM
  ) {
    if (key.length === 0 || key.includes(0))
      throw new YasProtocolError("empty Git named entity key");
  } else if (entityKind === g.YAS_GIT_ENTITY_STATUS) {
    const path = decodeFsPath(key);
    requireNonRootPath(path);
  } else if (entityKind === g.YAS_GIT_ENTITY_STASH) {
    if (key.length !== 4)
      throw new YasProtocolError("invalid Git stash entity key");
  } else if (!equal(key, new TextEncoder().encode("worktrees"))) {
    throw new YasProtocolError("invalid Git worktree generation key");
  }
  requireRevision(revision, "Git entity revision");
}

function validateHeadEntityBody(value: YasGitHeadEntityBody): void {
  if (
    value.flags & ~g.YAS_GIT_HEAD_FLAGS ||
    (value.flags & g.YAS_GIT_HEAD_FLAGS) === g.YAS_GIT_HEAD_FLAGS
  )
    throw new YasProtocolError("invalid Git HEAD flags");
  if (
    value.symbolicTarget.length > g.YAS_GIT_MAX_SPEC_BYTES ||
    value.symbolicTarget.includes(0)
  )
    throw new YasProtocolError("invalid Git symbolic target");
  const detached = Boolean(value.flags & g.YAS_GIT_HEAD_DETACHED);
  const unborn = Boolean(value.flags & g.YAS_GIT_HEAD_UNBORN);
  if (
    !(
      (detached &&
        !unborn &&
        value.object &&
        value.symbolicTarget.length === 0) ||
      (!detached &&
        unborn &&
        !value.object &&
        value.symbolicTarget.length !== 0) ||
      (!detached &&
        !unborn &&
        value.object &&
        value.symbolicTarget.length !== 0)
    )
  )
    throw new YasProtocolError("invalid Git HEAD entity state");
  if (value.object) validateObjectId(value.object);
}

function validateRefEntityBody(value: YasGitRefEntityBody): void {
  if (
    value.flags & ~g.YAS_GIT_REF_FLAGS ||
    Boolean(value.flags & g.YAS_GIT_REF_PEELED) !== Boolean(value.peeled) ||
    Boolean(value.flags & g.YAS_GIT_REF_SYMBOLIC) !==
      Boolean(value.symbolicTarget.length)
  )
    throw new YasProtocolError("invalid Git ref entity");
  if (
    value.symbolicTarget.length > g.YAS_GIT_MAX_SPEC_BYTES ||
    value.symbolicTarget.includes(0)
  )
    throw new YasProtocolError("invalid Git symbolic target");
  validateObjectId(value.object);
  if (value.peeled) validateObjectId(value.peeled);
}

function validateRemoteEntityBody(value: YasGitRemoteEntityBody): void {
  if (value.flags & ~g.YAS_GIT_REMOTE_FLAGS)
    throw new YasProtocolError("invalid Git remote flags");
  validateRaw(
    value.fetchUrl,
    g.YAS_GIT_MAX_REMOTE_BYTES,
    true,
    "Git fetch URL",
  );
  if (
    value.pushUrl.length > g.YAS_GIT_MAX_REMOTE_BYTES ||
    value.pushUrl.includes(0)
  )
    throw new YasProtocolError("invalid Git push URL");
  if (equal(value.fetchUrl, value.pushUrl))
    throw new YasProtocolError("Git push URL duplicates fetch URL");
}

function validateOperationEntityBody(value: YasGitOperationEntityBody): void {
  if (
    value.operationKind < g.YAS_GIT_OPERATION_MERGE ||
    value.operationKind > g.YAS_GIT_OPERATION_BISECT ||
    value.flags & ~g.YAS_GIT_OPERATION_FLAGS ||
    Boolean(value.flags & g.YAS_GIT_OPERATION_HEAD_PRESENT) !==
      Boolean(value.head) ||
    new TextEncoder().encode(value.detail).length >
      g.YAS_GIT_MAX_SUMMARY_BYTES ||
    value.detail.includes("\0")
  )
    throw new YasProtocolError("invalid Git operation entity");
  if (value.head) validateObjectId(value.head);
}

function validateStatusEntityBody(value: YasGitStatusEntityBody): void {
  if (
    value.indexStatus > g.YAS_GIT_WORKTREE_STATUS_IGNORED ||
    value.worktreeStatus > g.YAS_GIT_WORKTREE_STATUS_IGNORED ||
    value.flags & ~g.YAS_GIT_STATE_STATUS_FLAGS ||
    Boolean(value.flags & g.YAS_GIT_STATE_STATUS_CONTENT_PRESENT) !==
      Boolean(value.content) ||
    Boolean(value.flags & g.YAS_GIT_STATE_STATUS_OLD_PATH_PRESENT) !==
      Boolean(value.oldPath)
  )
    throw new YasProtocolError("invalid Git status entity");
  if (value.content) validateObjectId(value.content);
  if (value.oldPath) requireNonRootPath(value.oldPath);
}

function validateUpstreamEntityBody(value: YasGitUpstreamEntityBody): void {
  const gone = Boolean(value.flags & g.YAS_GIT_UPSTREAM_GONE);
  const countsValid = Boolean(value.flags & g.YAS_GIT_UPSTREAM_COUNTS_VALID);
  if (
    value.flags & ~g.YAS_GIT_UPSTREAM_FLAGS ||
    (gone && countsValid) ||
    (!countsValid && (value.ahead !== 0 || value.behind !== 0))
  )
    throw new YasProtocolError("invalid Git upstream entity");
  requireU32(value.ahead, "Git upstream ahead count");
  requireU32(value.behind, "Git upstream behind count");
  validateSpec(value.upstream);
}

function requireHandle(value: bigint, field: string): void {
  if (value === 0n) throw new YasProtocolError(`zero ${field}`);
}

function requireRevision(value: bigint, field: string): void {
  if (value === 0n) throw new YasProtocolError(`zero ${field}`);
}

function requireOperationId(value: Uint8Array): void {
  if (value.length !== 16 || value.every((byte) => byte === 0))
    throw new YasProtocolError("invalid Git operation ID");
}

function rejectRequired(
  extensions: readonly YasExtension[] | undefined,
  context: string,
): void {
  encodeExtensions(extensions);
  if (extensions?.some((extension) => extension.required))
    throw new YasProtocolError(`unknown required ${context} extension`);
}

function requireZero(value: Uint8Array, context: string): void {
  if (value.some((byte) => byte !== 0))
    throw new YasProtocolError(`${context} reserved bytes are nonzero`);
}

function equal(left: Uint8Array, right: Uint8Array): boolean {
  return (
    left.length === right.length &&
    left.every((byte, index) => byte === right[index])
  );
}

function entityKey(kind: number, key: Uint8Array): string {
  return `${kind}:${hex(key)}`;
}

function cloneQueryRecord(value: YasGitQueryRecord): YasGitQueryRecord {
  const cursor = new YasCursor(encodeGitQueryRecord(value));
  const clone = decodeGitQueryRecord(cursor);
  cursor.end("cloned Git query record");
  if (!clone) throw new YasProtocolError("known Git record was skipped");
  return clone;
}

function hex(value: Uint8Array): string {
  return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join(
    "",
  );
}
