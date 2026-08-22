/** YAS FS family v1 codecs and browser client. */

import * as g from "./generated";
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
  YasStateCatalogueRetention,
  YasStateSubscription,
  detachStateRetainedValue,
  encodeWatch,
  estimateStateRetainedBytes,
  negotiatedStateLimitU32,
  type YasStateBatch,
  type YasWatchOptions,
} from "./state";
import {
  YAS_TRANSFER_MODE_BYTE,
  YAS_TRANSFER_MODE_MESSAGE,
  YAS_TRANSFER_RECEIVER_TO_SENDER,
  YAS_TRANSFER_SENDER_TO_RECEIVER,
  decodeInlineOrTransfer,
  decodeTransferDescriptor,
  encodeInlineOrTransfer,
  encodeTransferDescriptor,
  requireTransferUploadStage,
  transfersFor,
  type YasInlineOrTransfer,
  type YasTransfer,
  type YasTransferDescriptor,
} from "./transfer";
import {
  YasCursor,
  YasProtocolError,
  YasWriter,
  decodeExtensions,
  encodeExtensions,
  type YasExtension,
  type YasTypedRecord,
} from "./wire";

export {
  YAS_FAMILY_FS,
  YAS_FS_APPLY,
  YAS_FS_CLOSE,
  YAS_FS_COMMIT,
  YAS_FS_FETCH,
  YAS_FS_GREP,
  YAS_FS_INDEX,
  YAS_FS_OPEN,
  YAS_FS_READ,
  YAS_FS_SEARCH,
  YAS_FS_STAGE_WRITE,
  YAS_FS_STATE,
  YAS_FS_STATE_ACK,
  YAS_FS_UNWATCH,
  YAS_FS_VERSION,
  YAS_FS_WATCH,
} from "./generated";

export interface YasFsPath {
  components: readonly Uint8Array[];
}

export type YasFsRootSource =
  | { kind: "platform-path"; path: Uint8Array }
  | { kind: "terminal-cwd"; terminalHandle: bigint; suffix: YasFsPath }
  | { kind: "process-cwd"; processHandle: bigint }
  | { kind: "staging" };

export interface YasFsOpen {
  flags: number;
  source: YasFsRootSource;
  extensions?: readonly YasExtension[];
}

export interface YasFsOpenResult {
  rootHandle: bigint;
  rootRevision: bigint;
  pathModel: number;
  caseBehavior: number;
  canonicalPath: Uint8Array;
  extensions: readonly YasExtension[];
}

export type YasFsEntryBody =
  | {
      kind: "file";
      byteLength: bigint;
      contentHash: Uint8Array;
      inlineContent?: Uint8Array;
    }
  | { kind: "directory" }
  | { kind: "symlink"; contentHash: Uint8Array; target: Uint8Array };

export interface YasFsEntryRecord {
  path: YasFsPath;
  entryRevision: bigint;
  flags: number;
  mode: number;
  modifiedUnixNs: bigint;
  body: YasFsEntryBody;
  extensions: readonly YasExtension[];
}

export interface YasFsEntryPatch {
  path: YasFsPath;
  observedRevision: bigint;
  fields: number;
  replacement: YasFsEntryRecord;
}

export interface YasFsMoveRecord {
  from: YasFsPath;
  to: YasFsPath;
  operationId?: Uint8Array;
}

export interface YasFsRemoveRecord {
  path: YasFsPath;
  removedRevision: bigint;
  operationId?: Uint8Array;
}

export interface YasFsSnapshot {
  revision: bigint;
  entries: readonly YasFsEntryRecord[];
}

export interface YasFsReadQuestion {
  kind: number;
  flags: number;
  path: YasFsPath;
}

export interface YasFsTypedRecord {
  kind: number;
  required: boolean;
  body: Uint8Array;
}

export interface YasFsQueryRecordBatch {
  firstRecordIndex: number;
  records: readonly YasFsTypedRecord[];
}

export type YasFsQueryDelivery =
  | { kind: "inline"; records: readonly YasFsTypedRecord[] }
  | { kind: "transfer"; descriptor: YasTransferDescriptor };

export interface YasFsQueryPageWire {
  nextCursor: Uint8Array;
  totalHint: bigint;
  flags: number;
  delivery: YasFsQueryDelivery;
  extensions: readonly YasExtension[];
}

export interface YasFsQueryPage {
  nextCursor: Uint8Array;
  totalHint: bigint;
  flags: number;
  records(): Promise<readonly YasFsTypedRecord[]>;
}

export interface YasFsQueryReadRecord {
  questionIndex: number;
  status: number;
  path?: YasFsPath;
  content: Uint8Array;
}

export interface YasFsQueryPathRecord {
  path: YasFsPath;
  flags: number;
}

export interface YasFsQueryGrepFileRecord {
  fileIndex: number;
  matchCount: number;
  flags: number;
  path: YasFsPath;
}

export interface YasFsQueryGrepMatchRecord {
  fileIndex: number;
  line: number;
  column: number;
  endLine: number;
  endColumn: number;
  text: string;
}

export type YasFsQueryRecord =
  | { kind: "read"; value: YasFsQueryReadRecord }
  | { kind: "path"; value: YasFsQueryPathRecord }
  | { kind: "grep-file"; value: YasFsQueryGrepFileRecord }
  | { kind: "grep-match"; value: YasFsQueryGrepMatchRecord }
  | { kind: "unknown"; recordKind: number; body: Uint8Array };

export type YasFsPrecondition =
  | { kind: "any" }
  | { kind: "absent" }
  | { kind: "revision"; revision: bigint }
  | { kind: "hash"; contentHash: Uint8Array };

export interface YasFsStageWrite {
  rootHandle: bigint;
  path: YasFsPath;
  precondition: YasFsPrecondition;
  flags: number;
  mode: number;
  byteLength: bigint;
  contentHash: Uint8Array;
  initialReceiveCredit: bigint;
  extensions?: readonly YasExtension[];
}

export interface YasFsStageWriteResult {
  stagingHandle: bigint;
  descriptor: YasTransferDescriptor;
  extensions: readonly YasExtension[];
}

export interface YasFsCommitResult {
  rootRevision: bigint;
  entryRevision: bigint;
  modifiedUnixNs: bigint;
  contentHash: Uint8Array;
}

export type YasFsApplyItem =
  | {
      kind: "write-inline";
      path: YasFsPath;
      precondition: YasFsPrecondition;
      createParents?: boolean;
      mode: number;
      content: Uint8Array;
    }
  | {
      kind: "mkdir";
      path: YasFsPath;
      precondition: YasFsPrecondition;
      createParents?: boolean;
      mode: number;
    }
  | {
      kind: "remove";
      path: YasFsPath;
      precondition: YasFsPrecondition;
      flags: number;
    }
  | {
      kind: "rename";
      from: YasFsPath;
      to: YasFsPath;
      precondition: YasFsPrecondition;
      createParents?: boolean;
    }
  | {
      kind: "symlink";
      path: YasFsPath;
      target: Uint8Array;
      precondition: YasFsPrecondition;
      createParents?: boolean;
    }
  | {
      kind: "hardlink";
      source: YasFsPath;
      target: YasFsPath;
      precondition: YasFsPrecondition;
      createParents?: boolean;
    };

export interface YasFsApply {
  rootHandle: bigint;
  operationId: Uint8Array;
  flags: number;
  items: readonly YasFsApplyItem[];
  extensions?: readonly YasExtension[];
}

export interface YasFsApplyItemResult {
  index: number;
  status: number;
  entryRevision: bigint;
  modifiedUnixNs: bigint;
  contentHash?: Uint8Array;
  detail: string;
}

export interface YasFsConflictDetail {
  path: YasFsPath;
  currentPresent: boolean;
  currentEntryRevision: bigint;
  modifiedUnixNs: bigint;
  currentHash?: Uint8Array;
}

export interface YasFsApplyResult {
  rootRevision: bigint;
  items: readonly YasFsApplyItemResult[];
  extensions: readonly YasExtension[];
}

export interface YasFsContent {
  byteLength: bigint;
  contentHash: Uint8Array;
  bytes(): Promise<Uint8Array>;
}

export function encodeFsPath(value: YasFsPath): Uint8Array {
  validatePath(value);
  const writer = new YasWriter().u16(value.components.length);
  for (const component of value.components) writer.bytesU16(component);
  return writer.finish();
}

export function decodeFsPath(bytes: Uint8Array): YasFsPath {
  const cursor = new YasCursor(bytes);
  const count = cursor.u16("FS path component count");
  if (
    count > g.YAS_FS_MAX_PATH_COMPONENTS ||
    count > Math.floor(cursor.remaining / 2)
  )
    throw new YasProtocolError("invalid FS path component count");
  const components: Uint8Array[] = [];
  for (let index = 0; index < count; index++)
    components.push(new Uint8Array(cursor.bytesU16("FS path component")));
  cursor.end("FS path");
  const value = { components };
  validatePath(value);
  return value;
}

export function encodeFsRootSource(value: YasFsRootSource): Uint8Array {
  validateRootSource(value);
  if (value.kind === "platform-path")
    return new YasWriter()
      .u8(g.YAS_FS_SOURCE_PLATFORM_PATH)
      .bytes(new Uint8Array(3))
      .bytesU32(value.path)
      .finish();
  if (value.kind === "terminal-cwd")
    return new YasWriter()
      .u8(g.YAS_FS_SOURCE_TERMINAL_CWD)
      .bytes(new Uint8Array(3))
      .u64(value.terminalHandle)
      .bytesU32(encodeFsPath(value.suffix))
      .finish();
  if (value.kind === "process-cwd")
    return new YasWriter()
      .u8(g.YAS_FS_SOURCE_PROCESS_CWD)
      .bytes(new Uint8Array(3))
      .u64(value.processHandle)
      .finish();
  return new YasWriter()
    .u8(g.YAS_FS_SOURCE_STAGING)
    .bytes(new Uint8Array(3))
    .finish();
}

export function decodeFsRootSource(bytes: Uint8Array): YasFsRootSource {
  const cursor = new YasCursor(bytes);
  const kind = cursor.u8("FS root source kind");
  requireZero(cursor.take(3, "FS source reserved"), "FS source");
  const value: YasFsRootSource =
    kind === g.YAS_FS_SOURCE_PLATFORM_PATH
      ? {
          kind: "platform-path",
          path: new Uint8Array(cursor.bytesU32("FS platform path")),
        }
      : kind === g.YAS_FS_SOURCE_TERMINAL_CWD
        ? {
            kind: "terminal-cwd",
            terminalHandle: cursor.u64("FS terminal"),
            suffix: decodeFsPath(cursor.bytesU32("FS terminal path suffix")),
          }
        : kind === g.YAS_FS_SOURCE_PROCESS_CWD
          ? { kind: "process-cwd", processHandle: cursor.u64("FS process") }
          : kind === g.YAS_FS_SOURCE_STAGING
            ? { kind: "staging" }
            : (() => {
                throw new YasProtocolError("unknown FS root source kind");
              })();
  cursor.end("FS root source");
  validateRootSource(value);
  return value;
}

export function encodeFsOpen(value: YasFsOpen): Uint8Array {
  if (value.flags & ~g.YAS_FS_OPEN_FLAGS)
    throw new YasProtocolError("invalid FS OPEN flags");
  return new YasWriter()
    .u16(value.flags)
    .u16(0)
    .bytesU32(encodeFsRootSource(value.source))
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeFsOpen(bytes: Uint8Array): YasFsOpen {
  const cursor = new YasCursor(bytes);
  const flags = cursor.u16("FS OPEN flags");
  if (cursor.u16("FS OPEN reserved") !== 0)
    throw new YasProtocolError("FS OPEN reserved field is nonzero");
  const value = {
    flags,
    source: decodeFsRootSource(cursor.bytesU32("FS root source")),
    extensions: decodeExtensions(cursor, new Set(), "FS OPEN extensions"),
  };
  cursor.end("FS OPEN");
  encodeFsOpen(value);
  return value;
}

export function encodeFsClose(
  rootHandle: bigint,
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  requireHandle(rootHandle, "FS root handle");
  return new YasWriter()
    .u64(rootHandle)
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function decodeFsClose(bytes: Uint8Array): {
  rootHandle: bigint;
  extensions: readonly YasExtension[];
} {
  const cursor = new YasCursor(bytes);
  const value = {
    rootHandle: cursor.u64("FS root handle"),
    extensions: decodeExtensions(cursor, new Set(), "FS CLOSE extensions"),
  };
  cursor.end("FS CLOSE");
  encodeFsClose(value.rootHandle, value.extensions);
  return value;
}

export function decodeFsOpenResult(bytes: Uint8Array): YasFsOpenResult {
  const cursor = new YasCursor(bytes);
  const rootHandle = cursor.u64("FS root handle");
  const rootRevision = cursor.u64("FS root revision");
  const pathModel = cursor.u8("FS path model");
  const caseBehavior = cursor.u8("FS case behavior");
  if (cursor.u16("FS OPEN Result reserved") !== 0)
    throw new YasProtocolError("FS OPEN Result reserved field is nonzero");
  const value = {
    rootHandle,
    rootRevision,
    pathModel,
    caseBehavior,
    canonicalPath: new Uint8Array(cursor.bytesU32("FS canonical path")),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "FS OPEN Result extensions",
    ),
  };
  cursor.end("FS OPEN Result");
  validateOpenResult(value);
  return value;
}

export function encodeFsWatch(
  rootHandle: bigint,
  flags: number,
  settleMs: number,
  inlineMax: number,
  ignorePatterns: string,
  encodedStateWatch: Uint8Array,
): Uint8Array {
  requireHandle(rootHandle, "FS root handle");
  if (
    flags & ~g.YAS_FS_WATCH_FLAGS ||
    !Number.isInteger(settleMs) ||
    settleMs < 0 ||
    settleMs > g.YAS_FS_MAX_WATCH_SETTLE_MS ||
    inlineMax > g.YAS_FS_MAX_INLINE_BYTES ||
    utf8Length(ignorePatterns) > g.YAS_FS_MAX_IGNORE_PATTERN_BYTES ||
    ignorePatterns.includes("\0")
  )
    throw new YasProtocolError("invalid FS WATCH policy or inline maximum");
  return new YasWriter()
    .u64(rootHandle)
    .u16(flags)
    .u16(settleMs)
    .u32(inlineMax)
    .utf8U32(ignorePatterns)
    .bytesU32(encodedStateWatch)
    .finish();
}

export function decodeFsWatch(bytes: Uint8Array): {
  rootHandle: bigint;
  flags: number;
  settleMs: number;
  inlineMax: number;
  ignorePatterns: string;
  encodedStateWatch: Uint8Array;
} {
  const cursor = new YasCursor(bytes);
  const rootHandle = cursor.u64("FS root handle");
  const flags = cursor.u16("FS WATCH flags");
  const settleMs = cursor.u16("FS WATCH settle milliseconds");
  const inlineMax = cursor.u32("FS WATCH inline maximum");
  const ignorePatterns = cursor.utf8U32("FS WATCH ignore patterns");
  const encodedStateWatch = new Uint8Array(cursor.bytesU32("FS State WATCH"));
  cursor.end("FS WATCH");
  encodeFsWatch(
    rootHandle,
    flags,
    settleMs,
    inlineMax,
    ignorePatterns,
    encodedStateWatch,
  );
  return {
    rootHandle,
    flags,
    settleMs,
    inlineMax,
    ignorePatterns,
    encodedStateWatch,
  };
}

export function encodeFsUnwatch(subscriptionId: number): Uint8Array {
  if (!Number.isInteger(subscriptionId) || subscriptionId <= 0)
    throw new YasProtocolError("invalid FS subscription ID");
  return new YasWriter().u32(subscriptionId).finish();
}

export function decodeFsUnwatch(bytes: Uint8Array): number {
  const cursor = new YasCursor(bytes);
  const subscriptionId = cursor.u32("FS subscription ID");
  cursor.end("FS UNWATCH");
  encodeFsUnwatch(subscriptionId);
  return subscriptionId;
}

export function encodeFsEntry(value: YasFsEntryRecord): Uint8Array {
  validateEntry(value);
  const kind =
    value.body.kind === "file"
      ? g.YAS_FS_ENTRY_FILE
      : value.body.kind === "directory"
        ? g.YAS_FS_ENTRY_DIRECTORY
        : g.YAS_FS_ENTRY_SYMLINK;
  const writer = new YasWriter()
    .bytesU32(encodeFsPath(value.path))
    .u64(value.entryRevision)
    .u8(kind)
    .u8(value.flags)
    .u16(0)
    .u32(value.mode)
    .i64(value.modifiedUnixNs);
  if (value.body.kind === "file") {
    writer
      .u64(value.body.byteLength)
      .bytes(value.body.contentHash)
      .u8(
        value.body.inlineContent === undefined
          ? g.YAS_FS_CONTENT_NONE
          : g.YAS_FS_CONTENT_INLINE,
      )
      .bytes(new Uint8Array(3));
    if (value.body.inlineContent !== undefined)
      writer.bytesU32(value.body.inlineContent);
  } else if (value.body.kind === "symlink")
    writer.bytes(value.body.contentHash).bytesU32(value.body.target);
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeFsEntry(bytes: Uint8Array): YasFsEntryRecord {
  const cursor = new YasCursor(bytes);
  const path = decodeFsPath(cursor.bytesU32("FS entry path"));
  const entryRevision = cursor.u64("FS entry revision");
  const kind = cursor.u8("FS entry kind");
  const flags = cursor.u8("FS entry flags");
  if (cursor.u16("FS entry reserved") !== 0)
    throw new YasProtocolError("FS entry reserved field is nonzero");
  const mode = cursor.u32("FS entry mode");
  const modifiedUnixNs = cursor.i64("FS modified time");
  let body: YasFsEntryBody;
  if (kind === g.YAS_FS_ENTRY_FILE) {
    const byteLength = cursor.u64("FS file byte length");
    const contentHash = new Uint8Array(cursor.take(32, "FS content hash"));
    const delivery = cursor.u8("FS content delivery");
    requireZero(cursor.take(3, "FS content reserved"), "FS content");
    body = {
      kind: "file",
      byteLength,
      contentHash,
      inlineContent:
        delivery === g.YAS_FS_CONTENT_INLINE
          ? new Uint8Array(cursor.bytesU32("FS inline content"))
          : delivery === g.YAS_FS_CONTENT_NONE
            ? undefined
            : (() => {
                throw new YasProtocolError("unknown FS content delivery");
              })(),
    };
  } else if (kind === g.YAS_FS_ENTRY_DIRECTORY) body = { kind: "directory" };
  else if (kind === g.YAS_FS_ENTRY_SYMLINK)
    body = {
      kind: "symlink",
      contentHash: new Uint8Array(cursor.take(32, "FS symlink content hash")),
      target: new Uint8Array(cursor.bytesU32("FS symlink target")),
    };
  else throw new YasProtocolError("unknown FS entry kind");
  const value = {
    path,
    entryRevision,
    flags,
    mode,
    modifiedUnixNs,
    body,
    extensions: decodeExtensions(
      cursor,
      new Set([g.YAS_FS_ENTRY_OPERATION_ID_EXTENSION]),
      "FS entry extensions",
    ),
  };
  cursor.end("FS entry");
  validateEntry(value);
  return value;
}

export function encodeFsEntryPatch(value: YasFsEntryPatch): Uint8Array {
  validatePath(value.path);
  requireRevision(value.observedRevision, "FS observed entry revision");
  if (
    value.fields === 0 ||
    value.fields & ~g.YAS_FS_PATCH_FIELDS ||
    !samePath(value.path, value.replacement.path)
  )
    throw new YasProtocolError("invalid FS entry patch");
  return new YasWriter()
    .bytesU32(encodeFsPath(value.path))
    .u64(value.observedRevision)
    .u16(value.fields)
    .u16(0)
    .bytesU32(encodeFsEntry(value.replacement))
    .finish();
}

export function decodeFsEntryPatch(bytes: Uint8Array): YasFsEntryPatch {
  const cursor = new YasCursor(bytes);
  const path = decodeFsPath(cursor.bytesU32("FS patch path"));
  const observedRevision = cursor.u64("FS observed entry revision");
  const fields = cursor.u16("FS patch fields");
  if (cursor.u16("FS patch reserved") !== 0)
    throw new YasProtocolError("FS patch reserved field is nonzero");
  const value = {
    path,
    observedRevision,
    fields,
    replacement: decodeFsEntry(cursor.bytesU32("FS patch replacement")),
  };
  cursor.end("FS entry patch");
  encodeFsEntryPatch(value);
  return value;
}

export function encodeFsMove(value: YasFsMoveRecord): Uint8Array {
  validatePath(value.from);
  validatePath(value.to);
  if (samePath(value.from, value.to))
    throw new YasProtocolError("FS MOVE paths are identical");
  if (value.operationId) requireOperationId(value.operationId);
  const writer = new YasWriter()
    .bytesU32(encodeFsPath(value.from))
    .bytesU32(encodeFsPath(value.to))
    .u8(value.operationId ? 1 : 0)
    .bytes(new Uint8Array(3));
  if (value.operationId) writer.bytes(value.operationId);
  return writer.finish();
}

export function decodeFsMove(bytes: Uint8Array): YasFsMoveRecord {
  const cursor = new YasCursor(bytes);
  const from = decodeFsPath(cursor.bytesU32("FS MOVE source"));
  const to = decodeFsPath(cursor.bytesU32("FS MOVE target"));
  const present = cursor.u8("FS MOVE operation presence");
  requireZero(cursor.take(3, "FS MOVE reserved"), "FS MOVE");
  if (present > 1)
    throw new YasProtocolError("invalid FS MOVE operation presence");
  const value = {
    from,
    to,
    operationId: present
      ? new Uint8Array(cursor.take(16, "FS operation ID"))
      : undefined,
  };
  cursor.end("FS MOVE");
  encodeFsMove(value);
  return value;
}

export function encodeFsRemoveRecord(value: YasFsRemoveRecord): Uint8Array {
  validatePath(value.path);
  requireRevision(value.removedRevision, "FS removed revision");
  if (value.operationId) requireOperationId(value.operationId);
  const writer = new YasWriter()
    .bytesU32(encodeFsPath(value.path))
    .u64(value.removedRevision)
    .u8(value.operationId ? 1 : 0)
    .bytes(new Uint8Array(3));
  if (value.operationId) writer.bytes(value.operationId);
  return writer.finish();
}

export function decodeFsRemoveRecord(bytes: Uint8Array): YasFsRemoveRecord {
  const cursor = new YasCursor(bytes);
  const path = decodeFsPath(cursor.bytesU32("FS REMOVE path"));
  const removedRevision = cursor.u64("FS removed revision");
  const present = cursor.u8("FS REMOVE operation presence");
  requireZero(cursor.take(3, "FS REMOVE reserved"), "FS REMOVE");
  if (present > 1)
    throw new YasProtocolError("invalid FS REMOVE operation presence");
  const value = {
    path,
    removedRevision,
    operationId: present
      ? new Uint8Array(cursor.take(16, "FS operation ID"))
      : undefined,
  };
  cursor.end("FS REMOVE");
  encodeFsRemoveRecord(value);
  return value;
}

export function encodeFsFetch(value: {
  rootHandle: bigint;
  path: YasFsPath;
  expectedHash?: Uint8Array;
  initialReceiveCredit: bigint;
  extensions?: readonly YasExtension[];
}): Uint8Array {
  requireHandle(value.rootHandle, "FS root handle");
  validatePath(value.path);
  if (value.expectedHash) requireHash(value.expectedHash);
  const writer = new YasWriter()
    .u64(value.rootHandle)
    .bytesU32(encodeFsPath(value.path))
    .u8(value.expectedHash ? 1 : 0)
    .bytes(new Uint8Array(3));
  if (value.expectedHash) writer.bytes(value.expectedHash);
  return writer
    .u64(value.initialReceiveCredit)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeFsFetch(bytes: Uint8Array): ReturnTypeShapeFsFetch {
  const cursor = new YasCursor(bytes);
  const rootHandle = cursor.u64("FS root handle");
  const path = decodeFsPath(cursor.bytesU32("FS FETCH path"));
  const present = cursor.u8("FS expected hash presence");
  requireZero(cursor.take(3, "FS FETCH reserved"), "FS FETCH");
  if (present > 1) throw new YasProtocolError("invalid FS FETCH hash presence");
  const value = {
    rootHandle,
    path,
    expectedHash: present
      ? new Uint8Array(cursor.take(32, "FS expected hash"))
      : undefined,
    initialReceiveCredit: cursor.u64("FS initial receive credit"),
    extensions: decodeExtensions(cursor, new Set(), "FS FETCH extensions"),
  };
  cursor.end("FS FETCH");
  encodeFsFetch(value);
  return value;
}

type ReturnTypeShapeFsFetch = {
  rootHandle: bigint;
  path: YasFsPath;
  expectedHash?: Uint8Array;
  initialReceiveCredit: bigint;
  extensions: readonly YasExtension[];
};

export function encodeFsContentResult(
  content: YasInlineOrTransfer,
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  validateFsContentDelivery(content);
  return new YasWriter()
    .bytesU32(encodeInlineOrTransfer(content))
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function decodeFsContentResult(bytes: Uint8Array): {
  content: YasInlineOrTransfer;
  extensions: readonly YasExtension[];
} {
  const cursor = new YasCursor(bytes);
  const content = decodeInlineOrTransfer(cursor.bytesU32("FS content"));
  const extensions = decodeExtensions(
    cursor,
    new Set(),
    "FS content extensions",
  );
  cursor.end("FS content Result");
  validateFsContentDelivery(content);
  return { content, extensions };
}

export function encodeFsRead(value: {
  rootHandle: bigint;
  initialReceiveCredit: bigint;
  questions: readonly YasFsReadQuestion[];
  extensions?: readonly YasExtension[];
}): Uint8Array {
  requireHandle(value.rootHandle, "FS root handle");
  if (
    value.questions.length === 0 ||
    value.questions.length > g.YAS_FS_MAX_QUERY_RECORDS
  )
    throw new YasProtocolError("invalid FS READ question count");
  const writer = new YasWriter()
    .u64(value.rootHandle)
    .u64(value.initialReceiveCredit)
    .u16(value.questions.length)
    .u16(0);
  for (const question of value.questions) {
    validateReadQuestion(question);
    writer
      .u16(question.kind)
      .u16(question.flags)
      .bytesU32(encodeFsPath(question.path));
  }
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeFsRead(bytes: Uint8Array): {
  rootHandle: bigint;
  initialReceiveCredit: bigint;
  questions: readonly YasFsReadQuestion[];
  extensions: readonly YasExtension[];
} {
  const cursor = new YasCursor(bytes);
  const rootHandle = cursor.u64("FS root handle");
  const initialReceiveCredit = cursor.u64("FS initial receive credit");
  const count = cursor.u16("FS READ question count");
  if (
    cursor.u16("FS READ reserved") !== 0 ||
    count === 0 ||
    count > g.YAS_FS_MAX_QUERY_RECORDS ||
    count > Math.floor(cursor.remaining / 8)
  )
    throw new YasProtocolError("invalid FS READ question count");
  const questions: YasFsReadQuestion[] = [];
  for (let index = 0; index < count; index++) {
    const question = {
      kind: cursor.u16("FS READ kind"),
      flags: cursor.u16("FS READ flags"),
      path: decodeFsPath(cursor.bytesU32("FS READ path")),
    };
    validateReadQuestion(question);
    questions.push(question);
  }
  const value = {
    rootHandle,
    initialReceiveCredit,
    questions,
    extensions: decodeExtensions(cursor, new Set(), "FS READ extensions"),
  };
  cursor.end("FS READ");
  encodeFsRead(value);
  return value;
}

export interface YasFsSearch {
  rootHandle: bigint;
  flags: number;
  maxResults: number;
  query: Uint8Array;
  cursor: Uint8Array;
  initialReceiveCredit: bigint;
  extensions?: readonly YasExtension[];
}

export function encodeFsSearch(value: YasFsSearch): Uint8Array {
  validateQuery(
    value.rootHandle,
    value.flags,
    g.YAS_FS_SEARCH_FLAGS,
    value.query,
    value.cursor,
  );
  return new YasWriter()
    .u64(value.rootHandle)
    .u16(value.flags)
    .u16(value.maxResults)
    .bytesU16(value.query)
    .bytesU16(value.cursor)
    .u64(value.initialReceiveCredit)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeFsSearch(bytes: Uint8Array): YasFsSearch {
  const cursor = new YasCursor(bytes);
  const value = {
    rootHandle: cursor.u64("FS root handle"),
    flags: cursor.u16("FS SEARCH flags"),
    maxResults: cursor.u16("FS SEARCH maximum results"),
    query: new Uint8Array(cursor.bytesU16("FS SEARCH query")),
    cursor: new Uint8Array(cursor.bytesU16("FS SEARCH cursor")),
    initialReceiveCredit: cursor.u64("FS initial receive credit"),
    extensions: decodeExtensions(cursor, new Set(), "FS SEARCH extensions"),
  };
  cursor.end("FS SEARCH");
  encodeFsSearch(value);
  return value;
}

export interface YasFsIndex {
  rootHandle: bigint;
  flags: number;
  maxResults: number;
  cursor: Uint8Array;
  initialReceiveCredit: bigint;
  extensions?: readonly YasExtension[];
}

export function encodeFsIndex(value: YasFsIndex): Uint8Array {
  validateQuery(
    value.rootHandle,
    value.flags,
    g.YAS_FS_INDEX_FLAGS,
    new Uint8Array([1]),
    value.cursor,
  );
  return new YasWriter()
    .u64(value.rootHandle)
    .u16(value.flags)
    .u16(value.maxResults)
    .bytesU16(value.cursor)
    .u64(value.initialReceiveCredit)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeFsIndex(bytes: Uint8Array): YasFsIndex {
  const cursor = new YasCursor(bytes);
  const value = {
    rootHandle: cursor.u64("FS root handle"),
    flags: cursor.u16("FS INDEX flags"),
    maxResults: cursor.u16("FS INDEX maximum results"),
    cursor: new Uint8Array(cursor.bytesU16("FS INDEX cursor")),
    initialReceiveCredit: cursor.u64("FS initial receive credit"),
    extensions: decodeExtensions(cursor, new Set(), "FS INDEX extensions"),
  };
  cursor.end("FS INDEX");
  encodeFsIndex(value);
  return value;
}

export interface YasFsGrep extends YasFsSearch {
  maxPerFile: number;
}

export function encodeFsGrep(value: YasFsGrep): Uint8Array {
  validateQuery(
    value.rootHandle,
    value.flags,
    g.YAS_FS_GREP_FLAGS,
    value.query,
    value.cursor,
  );
  return new YasWriter()
    .u64(value.rootHandle)
    .u16(value.flags)
    .u16(value.maxResults)
    .u16(value.maxPerFile)
    .u16(0)
    .bytesU32(value.query)
    .bytesU16(value.cursor)
    .u64(value.initialReceiveCredit)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeFsGrep(bytes: Uint8Array): YasFsGrep {
  const cursor = new YasCursor(bytes);
  const rootHandle = cursor.u64("FS root handle");
  const flags = cursor.u16("FS GREP flags");
  const maxResults = cursor.u16("FS GREP maximum results");
  const maxPerFile = cursor.u16("FS GREP maximum per file");
  if (cursor.u16("FS GREP reserved") !== 0)
    throw new YasProtocolError("FS GREP reserved field is nonzero");
  const value = {
    rootHandle,
    flags,
    maxResults,
    maxPerFile,
    query: new Uint8Array(cursor.bytesU32("FS GREP query")),
    cursor: new Uint8Array(cursor.bytesU16("FS GREP cursor")),
    initialReceiveCredit: cursor.u64("FS initial receive credit"),
    extensions: decodeExtensions(cursor, new Set(), "FS GREP extensions"),
  };
  cursor.end("FS GREP");
  encodeFsGrep(value);
  return value;
}

export function encodeFsTypedRecord(value: YasFsTypedRecord): Uint8Array {
  const bodyLength = value.body.length + 4;
  return new YasWriter()
    .u32(bodyLength)
    .u16(value.kind)
    .u16(value.required ? 1 : 0)
    .bytes(value.body)
    .finish();
}

function decodeFsTypedRecordFrom(cursor: YasCursor): YasFsTypedRecord {
  const bytes = cursor.bytesU32("FS typed record");
  const record = new YasCursor(bytes);
  const kind = record.u16("FS record kind");
  const flags = record.u16("FS record flags");
  if (flags & ~1) throw new YasProtocolError("invalid FS typed record flags");
  const value = {
    kind,
    required: Boolean(flags & 1),
    body: new Uint8Array(record.take(record.remaining, "FS record body")),
  };
  record.end("FS typed record");
  return value;
}

export function decodeFsRecordStream(bytes: Uint8Array): YasFsTypedRecord[] {
  const cursor = new YasCursor(bytes);
  const records: YasFsTypedRecord[] = [];
  while (cursor.remaining !== 0) {
    if (records.length >= g.YAS_FS_MAX_QUERY_RECORDS)
      throw new YasProtocolError("FS query record count exceeds its limit");
    records.push(decodeFsTypedRecordFrom(cursor));
  }
  return records;
}

export function encodeFsRecordStream(
  records: readonly YasFsTypedRecord[],
): Uint8Array {
  if (records.length > g.YAS_FS_MAX_QUERY_RECORDS)
    throw new YasProtocolError("FS query record count exceeds its limit");
  for (const record of records) decodeFsQueryRecord(record);
  const encoded = concat(records.map(encodeFsTypedRecord));
  if (encoded.length > g.YAS_FS_MAX_QUERY_BYTES)
    throw new YasProtocolError("FS query record bytes exceed their limit");
  return encoded;
}

export function encodeFsQueryRecordBatch(
  value: YasFsQueryRecordBatch,
): Uint8Array {
  if (
    !u32(value.firstRecordIndex) ||
    value.records.length === 0 ||
    value.records.length > g.YAS_FS_MAX_QUERY_RECORDS
  )
    throw new YasProtocolError("invalid FS query batch record count");
  validateFsQueryRecords(value.records, false);
  return new YasWriter()
    .u32(value.firstRecordIndex)
    .u16(value.records.length)
    .u16(0)
    .bytesU32(encodeFsRecordStream(value.records))
    .finish();
}

export function decodeFsQueryRecordBatch(
  bytes: Uint8Array,
): YasFsQueryRecordBatch {
  const cursor = new YasCursor(bytes);
  const firstRecordIndex = cursor.u32("FS query batch first record index");
  const count = cursor.u16("FS query batch record count");
  if (count === 0 || cursor.u16("FS query batch reserved") !== 0)
    throw new YasProtocolError(
      "invalid FS query batch count or reserved field",
    );
  const records = decodeFsRecordStream(
    cursor.bytesU32("FS query batch records"),
  );
  if (records.length !== count)
    throw new YasProtocolError("FS query batch record count mismatch");
  const value = { firstRecordIndex, records };
  cursor.end("FS query record batch");
  encodeFsQueryRecordBatch(value);
  return value;
}

export function encodeFsQueryReadRecord(
  value: YasFsQueryReadRecord,
): Uint8Array {
  if (
    !u16(value.questionIndex) ||
    !u16(value.status) ||
    value.status > g.YAS_STATUS_INTERNAL ||
    (value.status === g.YAS_STATUS_OK && value.path === undefined) ||
    (value.status !== g.YAS_STATUS_OK && value.content.length !== 0) ||
    value.content.length > g.YAS_FS_MAX_QUERY_BYTES
  )
    throw new YasProtocolError("invalid FS READ result record");
  const writer = new YasWriter()
    .u16(value.questionIndex)
    .u16(value.status)
    .u8(value.path ? 1 : 0)
    .bytes(new Uint8Array(3));
  if (value.path) writer.bytesU32(encodeFsPath(value.path));
  return writer.bytesU32(value.content).finish();
}

export function decodeFsQueryReadRecord(
  bytes: Uint8Array,
): YasFsQueryReadRecord {
  const cursor = new YasCursor(bytes);
  const questionIndex = cursor.u16("FS READ result question index");
  const status = cursor.u16("FS READ result status");
  const present = cursor.u8("FS READ result path presence");
  if (present > 1)
    throw new YasProtocolError("invalid FS READ result path presence");
  requireZero(cursor.take(3, "FS READ result reserved"), "FS READ result");
  const value = {
    questionIndex,
    status,
    path: present
      ? decodeFsPath(cursor.bytesU32("FS READ result path"))
      : undefined,
    content: new Uint8Array(cursor.bytesU32("FS READ result content")),
  };
  cursor.end("FS READ result record");
  encodeFsQueryReadRecord(value);
  return value;
}

export function encodeFsQueryPathRecord(
  value: YasFsQueryPathRecord,
): Uint8Array {
  if (value.flags & ~g.YAS_FS_QUERY_PATH_FLAGS)
    throw new YasProtocolError("invalid FS query path flags");
  return new YasWriter()
    .bytesU32(encodeFsPath(value.path))
    .u16(value.flags)
    .u16(0)
    .finish();
}

export function decodeFsQueryPathRecord(
  bytes: Uint8Array,
): YasFsQueryPathRecord {
  const cursor = new YasCursor(bytes);
  const value = {
    path: decodeFsPath(cursor.bytesU32("FS query path")),
    flags: cursor.u16("FS query path flags"),
  };
  if (cursor.u16("FS query path reserved") !== 0)
    throw new YasProtocolError("FS query path reserved field is nonzero");
  cursor.end("FS query path record");
  encodeFsQueryPathRecord(value);
  return value;
}

export function encodeFsQueryGrepFileRecord(
  value: YasFsQueryGrepFileRecord,
): Uint8Array {
  if (
    !u32(value.fileIndex) ||
    !u32(value.matchCount) ||
    value.matchCount > g.YAS_FS_MAX_QUERY_RECORDS ||
    value.flags & ~g.YAS_FS_QUERY_GREP_FILE_FLAGS
  )
    throw new YasProtocolError("invalid FS GREP file record");
  return new YasWriter()
    .u32(value.fileIndex)
    .u32(value.matchCount)
    .u16(value.flags)
    .u16(0)
    .bytesU32(encodeFsPath(value.path))
    .finish();
}

export function decodeFsQueryGrepFileRecord(
  bytes: Uint8Array,
): YasFsQueryGrepFileRecord {
  const cursor = new YasCursor(bytes);
  const fileIndex = cursor.u32("FS GREP file index");
  const matchCount = cursor.u32("FS GREP match count");
  const flags = cursor.u16("FS GREP file flags");
  if (cursor.u16("FS GREP file reserved") !== 0)
    throw new YasProtocolError("FS GREP file reserved field is nonzero");
  const value = {
    fileIndex,
    matchCount,
    flags,
    path: decodeFsPath(cursor.bytesU32("FS GREP file path")),
  };
  cursor.end("FS GREP file record");
  encodeFsQueryGrepFileRecord(value);
  return value;
}

export function encodeFsQueryGrepMatchRecord(
  value: YasFsQueryGrepMatchRecord,
): Uint8Array {
  if (
    !u32(value.fileIndex) ||
    !u32(value.line) ||
    !u32(value.column) ||
    !u32(value.endLine) ||
    !u32(value.endColumn) ||
    value.endLine < value.line ||
    (value.endLine === value.line && value.endColumn < value.column) ||
    utf8Length(value.text) > g.YAS_FS_MAX_GREP_LINE_BYTES ||
    value.text.includes("\0")
  )
    throw new YasProtocolError("invalid FS GREP match record");
  return new YasWriter()
    .u32(value.fileIndex)
    .u32(value.line)
    .u32(value.column)
    .u32(value.endLine)
    .u32(value.endColumn)
    .utf8U32(value.text)
    .finish();
}

export function decodeFsQueryGrepMatchRecord(
  bytes: Uint8Array,
): YasFsQueryGrepMatchRecord {
  const cursor = new YasCursor(bytes);
  const value = {
    fileIndex: cursor.u32("FS GREP match file index"),
    line: cursor.u32("FS GREP match line"),
    column: cursor.u32("FS GREP match column"),
    endLine: cursor.u32("FS GREP match end line"),
    endColumn: cursor.u32("FS GREP match end column"),
    text: cursor.utf8U32("FS GREP match text"),
  };
  cursor.end("FS GREP match record");
  encodeFsQueryGrepMatchRecord(value);
  return value;
}

export function encodeFsQueryRecord(value: YasFsQueryRecord): YasFsTypedRecord {
  if (value.kind === "read")
    return {
      kind: g.YAS_FS_QUERY_RECORD_READ,
      required: false,
      body: encodeFsQueryReadRecord(value.value),
    };
  if (value.kind === "path")
    return {
      kind: g.YAS_FS_QUERY_RECORD_PATH,
      required: false,
      body: encodeFsQueryPathRecord(value.value),
    };
  if (value.kind === "grep-file")
    return {
      kind: g.YAS_FS_QUERY_RECORD_GREP_FILE,
      required: false,
      body: encodeFsQueryGrepFileRecord(value.value),
    };
  if (value.kind === "grep-match")
    return {
      kind: g.YAS_FS_QUERY_RECORD_GREP_MATCH,
      required: false,
      body: encodeFsQueryGrepMatchRecord(value.value),
    };
  return {
    kind: value.recordKind,
    required: false,
    body: new Uint8Array(value.body),
  };
}

export function decodeFsQueryRecord(
  record: YasFsTypedRecord,
): YasFsQueryRecord {
  if (record.kind === g.YAS_FS_QUERY_RECORD_READ)
    return { kind: "read", value: decodeFsQueryReadRecord(record.body) };
  if (record.kind === g.YAS_FS_QUERY_RECORD_PATH)
    return { kind: "path", value: decodeFsQueryPathRecord(record.body) };
  if (record.kind === g.YAS_FS_QUERY_RECORD_GREP_FILE)
    return {
      kind: "grep-file",
      value: decodeFsQueryGrepFileRecord(record.body),
    };
  if (record.kind === g.YAS_FS_QUERY_RECORD_GREP_MATCH)
    return {
      kind: "grep-match",
      value: decodeFsQueryGrepMatchRecord(record.body),
    };
  if (record.required)
    throw new YasProtocolError("unknown required FS query record");
  return {
    kind: "unknown",
    recordKind: record.kind,
    body: new Uint8Array(record.body),
  };
}

export function encodeFsQueryPage(value: YasFsQueryPageWire): Uint8Array {
  validateQueryPage(value);
  const writer = new YasWriter()
    .bytesU16(value.nextCursor)
    .u64(value.totalHint)
    .u16(value.flags)
    .u16(0);
  if (value.delivery.kind === "inline") {
    const records = concat(value.delivery.records.map(encodeFsTypedRecord));
    writer
      .u8(g.YAS_FS_PAGE_INLINE)
      .bytes(new Uint8Array(3))
      .u16(value.delivery.records.length)
      .u16(0)
      .bytesU32(records);
  } else
    writer
      .u8(g.YAS_FS_PAGE_TRANSFER)
      .bytes(new Uint8Array(3))
      .bytesU32(encodeTransferDescriptor(value.delivery.descriptor));
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeFsQueryPage(bytes: Uint8Array): YasFsQueryPageWire {
  const cursor = new YasCursor(bytes);
  const nextCursor = new Uint8Array(cursor.bytesU16("FS next cursor"));
  const totalHint = cursor.u64("FS total hint");
  const flags = cursor.u16("FS query page flags");
  if (cursor.u16("FS query page reserved") !== 0)
    throw new YasProtocolError("FS query page reserved field is nonzero");
  const kind = cursor.u8("FS query delivery");
  requireZero(
    cursor.take(3, "FS query delivery reserved"),
    "FS query delivery",
  );
  let delivery: YasFsQueryDelivery;
  if (kind === g.YAS_FS_PAGE_INLINE) {
    const count = cursor.u16("FS query record count");
    if (cursor.u16("FS query record reserved") !== 0)
      throw new YasProtocolError("FS query record reserved field is nonzero");
    const recordCursor = new YasCursor(
      cursor.bytesU32("FS query record stream"),
    );
    const records: YasFsTypedRecord[] = [];
    for (let index = 0; index < count; index++)
      records.push(decodeFsTypedRecordFrom(recordCursor));
    recordCursor.end("FS query record stream");
    delivery = { kind: "inline", records };
  } else if (kind === g.YAS_FS_PAGE_TRANSFER)
    delivery = {
      kind: "transfer",
      descriptor: decodeDescriptor(cursor.bytesU32("FS query Transfer")),
    };
  else throw new YasProtocolError("unknown FS query delivery");
  const value = {
    nextCursor,
    totalHint,
    flags,
    delivery,
    extensions: decodeExtensions(cursor, new Set(), "FS query page extensions"),
  };
  cursor.end("FS query page");
  validateQueryPage(value);
  return value;
}

export function encodeFsPrecondition(value: YasFsPrecondition): Uint8Array {
  const kind =
    value.kind === "any"
      ? g.YAS_FS_PRECONDITION_ANY
      : value.kind === "absent"
        ? g.YAS_FS_PRECONDITION_ABSENT
        : value.kind === "revision"
          ? g.YAS_FS_PRECONDITION_REVISION
          : g.YAS_FS_PRECONDITION_HASH;
  const writer = new YasWriter().u8(kind).bytes(new Uint8Array(3));
  if (value.kind === "revision") {
    requireRevision(value.revision, "FS precondition revision");
    writer.u64(value.revision);
  } else if (value.kind === "hash") {
    requireHash(value.contentHash);
    writer.bytes(value.contentHash);
  }
  return writer.finish();
}

export function decodeFsPrecondition(bytes: Uint8Array): YasFsPrecondition {
  const cursor = new YasCursor(bytes);
  const kind = cursor.u8("FS precondition kind");
  requireZero(cursor.take(3, "FS precondition reserved"), "FS precondition");
  const value: YasFsPrecondition =
    kind === g.YAS_FS_PRECONDITION_ANY
      ? { kind: "any" }
      : kind === g.YAS_FS_PRECONDITION_ABSENT
        ? { kind: "absent" }
        : kind === g.YAS_FS_PRECONDITION_REVISION
          ? {
              kind: "revision",
              revision: cursor.u64("FS precondition revision"),
            }
          : kind === g.YAS_FS_PRECONDITION_HASH
            ? {
                kind: "hash",
                contentHash: new Uint8Array(
                  cursor.take(32, "FS precondition hash"),
                ),
              }
            : (() => {
                throw new YasProtocolError("unknown FS precondition kind");
              })();
  cursor.end("FS precondition");
  encodeFsPrecondition(value);
  return value;
}

export function encodeFsConflictDetail(value: YasFsConflictDetail): Uint8Array {
  validatePath(value.path);
  if (
    (value.currentPresent && value.currentEntryRevision === 0n) ||
    (!value.currentPresent &&
      (value.currentEntryRevision !== 0n ||
        value.modifiedUnixNs !== 0n ||
        value.currentHash !== undefined))
  )
    throw new YasProtocolError("invalid FS conflict current entry");
  if (value.currentHash) requireHash(value.currentHash);
  const writer = new YasWriter()
    .bytesU32(encodeFsPath(value.path))
    .u8(value.currentPresent ? 1 : 0)
    .u8(value.currentHash ? 1 : 0)
    .u16(0)
    .u64(value.currentEntryRevision)
    .i64(value.modifiedUnixNs);
  if (value.currentHash) writer.bytes(value.currentHash);
  return writer.finish();
}

export function decodeFsConflictDetail(bytes: Uint8Array): YasFsConflictDetail {
  const cursor = new YasCursor(bytes);
  const path = decodeFsPath(cursor.bytesU32("FS conflict path"));
  const currentPresent = cursor.u8("FS conflict current presence");
  const hashPresent = cursor.u8("FS conflict hash presence");
  if (currentPresent > 1 || hashPresent > 1)
    throw new YasProtocolError("invalid FS conflict presence");
  if (cursor.u16("FS conflict reserved") !== 0)
    throw new YasProtocolError("FS conflict reserved field is nonzero");
  const value = {
    path,
    currentPresent: Boolean(currentPresent),
    currentEntryRevision: cursor.u64("FS conflict current revision"),
    modifiedUnixNs: cursor.i64("FS conflict modified time"),
    currentHash: hashPresent
      ? new Uint8Array(cursor.take(32, "FS conflict current hash"))
      : undefined,
  };
  cursor.end("FS conflict detail");
  encodeFsConflictDetail(value);
  return value;
}

export function encodeFsStageWrite(value: YasFsStageWrite): Uint8Array {
  requireHandle(value.rootHandle, "FS root handle");
  validatePath(value.path);
  requireHash(value.contentHash);
  if (
    value.flags & ~g.YAS_FS_STAGE_FLAGS ||
    value.byteLength > BigInt(g.YAS_FS_MAX_STAGED_BYTES)
  )
    throw new YasProtocolError("FS staged bytes exceed their limit");
  return new YasWriter()
    .u64(value.rootHandle)
    .bytesU32(encodeFsPath(value.path))
    .bytesU32(encodeFsPrecondition(value.precondition))
    .u16(value.flags)
    .u16(0)
    .u32(value.mode)
    .u64(value.byteLength)
    .bytes(value.contentHash)
    .u64(value.initialReceiveCredit)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeFsStageWrite(bytes: Uint8Array): YasFsStageWrite {
  const cursor = new YasCursor(bytes);
  const value = {
    rootHandle: cursor.u64("FS root handle"),
    path: decodeFsPath(cursor.bytesU32("FS staged path")),
    precondition: decodeFsPrecondition(cursor.bytesU32("FS precondition")),
    flags: cursor.u16("FS STAGE_WRITE flags"),
    reserved: cursor.u16("FS STAGE_WRITE reserved"),
    mode: cursor.u32("FS staged mode"),
    byteLength: cursor.u64("FS staged byte length"),
    contentHash: new Uint8Array(cursor.take(32, "FS content hash")),
    initialReceiveCredit: cursor.u64("FS initial receive credit"),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "FS STAGE_WRITE extensions",
    ),
  };
  if (value.reserved !== 0)
    throw new YasProtocolError("FS STAGE_WRITE reserved field is nonzero");
  cursor.end("FS STAGE_WRITE");
  const { reserved: _reserved, ...decoded } = value;
  encodeFsStageWrite(decoded);
  return decoded;
}

export function encodeFsStageWriteResult(
  value: YasFsStageWriteResult,
): Uint8Array {
  requireHandle(value.stagingHandle, "FS staging handle");
  validateFsTransfer(
    value.descriptor,
    g.YAS_FS_STAGED_WRITE_CONTENT_KIND,
    YAS_TRANSFER_MODE_BYTE,
    YAS_TRANSFER_RECEIVER_TO_SENDER,
  );
  requireTransferUploadStage(
    value.descriptor,
    value.stagingHandle,
    "FS staged-write descriptor",
  );
  return new YasWriter()
    .u64(value.stagingHandle)
    .bytesU32(encodeTransferDescriptor(value.descriptor))
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeFsStageWriteResult(
  bytes: Uint8Array,
): YasFsStageWriteResult {
  const cursor = new YasCursor(bytes);
  const value = {
    stagingHandle: cursor.u64("FS staging handle"),
    descriptor: decodeDescriptor(cursor.bytesU32("FS staged Transfer")),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "FS stage Result extensions",
    ),
  };
  cursor.end("FS STAGE_WRITE Result");
  encodeFsStageWriteResult(value);
  return value;
}

export function encodeFsCommit(value: {
  stagingHandle: bigint;
  operationId: Uint8Array;
  flags: number;
  extensions?: readonly YasExtension[];
}): Uint8Array {
  requireHandle(value.stagingHandle, "FS staging handle");
  requireOperationId(value.operationId);
  if (value.flags & ~g.YAS_FS_COMMIT_FLAGS)
    throw new YasProtocolError("invalid FS COMMIT flags");
  return new YasWriter()
    .u64(value.stagingHandle)
    .bytes(value.operationId)
    .u16(value.flags)
    .u16(0)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeFsCommit(bytes: Uint8Array): {
  stagingHandle: bigint;
  operationId: Uint8Array;
  flags: number;
  extensions: readonly YasExtension[];
} {
  const cursor = new YasCursor(bytes);
  const stagingHandle = cursor.u64("FS staging handle");
  const operationId = new Uint8Array(cursor.take(16, "FS operation ID"));
  const flags = cursor.u16("FS COMMIT flags");
  if (cursor.u16("FS COMMIT reserved") !== 0)
    throw new YasProtocolError("FS COMMIT reserved field is nonzero");
  const value = {
    stagingHandle,
    operationId,
    flags,
    extensions: decodeExtensions(cursor, new Set(), "FS COMMIT extensions"),
  };
  cursor.end("FS COMMIT");
  encodeFsCommit(value);
  return value;
}

export function encodeFsCommitResult(value: YasFsCommitResult): Uint8Array {
  requireRevision(value.rootRevision, "FS root revision");
  requireRevision(value.entryRevision, "FS entry revision");
  requireHash(value.contentHash);
  return new YasWriter()
    .u64(value.rootRevision)
    .u64(value.entryRevision)
    .i64(value.modifiedUnixNs)
    .bytes(value.contentHash)
    .finish();
}

export function decodeFsCommitResult(bytes: Uint8Array): YasFsCommitResult {
  const cursor = new YasCursor(bytes);
  const value = {
    rootRevision: cursor.u64("FS root revision"),
    entryRevision: cursor.u64("FS entry revision"),
    modifiedUnixNs: cursor.i64("FS modified time"),
    contentHash: new Uint8Array(cursor.take(32, "FS content hash")),
  };
  cursor.end("FS COMMIT Result");
  requireRevision(value.rootRevision, "FS root revision");
  requireRevision(value.entryRevision, "FS entry revision");
  encodeFsCommitResult(value);
  return value;
}

function encodeFsApplyItem(value: YasFsApplyItem): Uint8Array {
  const body = new YasWriter();
  let kind: number;
  const itemFlags =
    value.kind !== "remove" && value.createParents
      ? g.YAS_FS_APPLY_ITEM_CREATE_PARENTS
      : 0;
  if (value.kind === "write-inline") {
    if (value.content.length > g.YAS_FS_MAX_INLINE_BYTES)
      throw new YasProtocolError("FS inline apply content exceeds its limit");
    kind = g.YAS_FS_APPLY_WRITE_INLINE;
    body
      .bytesU32(encodeFsPath(value.path))
      .bytesU32(encodeFsPrecondition(value.precondition))
      .u32(value.mode)
      .bytesU32(value.content);
  } else if (value.kind === "mkdir") {
    kind = g.YAS_FS_APPLY_MKDIR;
    body
      .bytesU32(encodeFsPath(value.path))
      .bytesU32(encodeFsPrecondition(value.precondition))
      .u32(value.mode);
  } else if (value.kind === "remove") {
    if (value.flags & ~g.YAS_FS_REMOVE_FLAGS)
      throw new YasProtocolError("invalid FS APPLY remove flags");
    kind = g.YAS_FS_APPLY_REMOVE;
    body
      .bytesU32(encodeFsPath(value.path))
      .bytesU32(encodeFsPrecondition(value.precondition))
      .u16(value.flags)
      .u16(0);
  } else if (value.kind === "rename") {
    if (samePath(value.from, value.to))
      throw new YasProtocolError("FS APPLY rename paths are identical");
    kind = g.YAS_FS_APPLY_RENAME;
    body
      .bytesU32(encodeFsPath(value.from))
      .bytesU32(encodeFsPath(value.to))
      .bytesU32(encodeFsPrecondition(value.precondition));
  } else if (value.kind === "symlink") {
    if (
      value.target.length === 0 ||
      value.target.length > g.YAS_FS_MAX_PATH_BYTES ||
      value.target.includes(0)
    )
      throw new YasProtocolError("invalid FS APPLY symlink target");
    kind = g.YAS_FS_APPLY_SYMLINK;
    body
      .bytesU32(encodeFsPath(value.path))
      .bytesU32(value.target)
      .bytesU32(encodeFsPrecondition(value.precondition));
  } else {
    if (samePath(value.source, value.target))
      throw new YasProtocolError("FS APPLY hardlink paths are identical");
    kind = g.YAS_FS_APPLY_HARDLINK;
    body
      .bytesU32(encodeFsPath(value.source))
      .bytesU32(encodeFsPath(value.target))
      .bytesU32(encodeFsPrecondition(value.precondition));
  }
  const encoded = body.finish();
  return new YasWriter()
    .u32(encoded.length + 4)
    .u16(kind)
    .u16(itemFlags)
    .bytes(encoded)
    .finish();
}

function decodeFsApplyItem(cursor: YasCursor): YasFsApplyItem {
  const bytes = cursor.bytesU32("FS APPLY item");
  const item = new YasCursor(bytes);
  const kind = item.u16("FS APPLY item kind");
  const itemFlags = item.u16("FS APPLY item flags");
  if (itemFlags & ~g.YAS_FS_APPLY_ITEM_FLAGS)
    throw new YasProtocolError("FS APPLY item flags are invalid");
  const createParents = Boolean(itemFlags & g.YAS_FS_APPLY_ITEM_CREATE_PARENTS);
  let value: YasFsApplyItem;
  if (kind === g.YAS_FS_APPLY_WRITE_INLINE)
    value = {
      kind: "write-inline",
      path: decodeFsPath(item.bytesU32("FS APPLY path")),
      precondition: decodeFsPrecondition(item.bytesU32("FS precondition")),
      createParents,
      mode: item.u32("FS mode"),
      content: new Uint8Array(item.bytesU32("FS inline content")),
    };
  else if (kind === g.YAS_FS_APPLY_MKDIR)
    value = {
      kind: "mkdir",
      path: decodeFsPath(item.bytesU32("FS APPLY path")),
      precondition: decodeFsPrecondition(item.bytesU32("FS precondition")),
      createParents,
      mode: item.u32("FS mode"),
    };
  else if (kind === g.YAS_FS_APPLY_REMOVE) {
    const path = decodeFsPath(item.bytesU32("FS APPLY path"));
    const precondition = decodeFsPrecondition(item.bytesU32("FS precondition"));
    const flags = item.u16("FS remove flags");
    if (item.u16("FS remove reserved") !== 0)
      throw new YasProtocolError("FS remove reserved field is nonzero");
    if (createParents)
      throw new YasProtocolError("FS APPLY remove cannot create parents");
    value = { kind: "remove", path, precondition, flags };
  } else if (kind === g.YAS_FS_APPLY_RENAME)
    value = {
      kind: "rename",
      from: decodeFsPath(item.bytesU32("FS rename source")),
      to: decodeFsPath(item.bytesU32("FS rename target")),
      precondition: decodeFsPrecondition(item.bytesU32("FS precondition")),
      createParents,
    };
  else if (kind === g.YAS_FS_APPLY_SYMLINK)
    value = {
      kind: "symlink",
      path: decodeFsPath(item.bytesU32("FS symlink path")),
      target: new Uint8Array(item.bytesU32("FS symlink target")),
      precondition: decodeFsPrecondition(item.bytesU32("FS precondition")),
      createParents,
    };
  else if (kind === g.YAS_FS_APPLY_HARDLINK)
    value = {
      kind: "hardlink",
      source: decodeFsPath(item.bytesU32("FS hardlink source")),
      target: decodeFsPath(item.bytesU32("FS hardlink target")),
      precondition: decodeFsPrecondition(item.bytesU32("FS precondition")),
      createParents,
    };
  else throw new YasProtocolError("unknown FS APPLY item kind");
  item.end("FS APPLY item");
  encodeFsApplyItem(value);
  return value;
}

export function encodeFsApply(value: YasFsApply): Uint8Array {
  requireHandle(value.rootHandle, "FS root handle");
  requireOperationId(value.operationId);
  if (
    value.flags & ~g.YAS_FS_APPLY_FLAGS ||
    value.items.length === 0 ||
    value.items.length > g.YAS_FS_MAX_BATCH_ITEMS
  )
    throw new YasProtocolError("invalid FS APPLY flags or item count");
  const writer = new YasWriter()
    .u64(value.rootHandle)
    .bytes(value.operationId)
    .u16(value.flags)
    .u16(value.items.length);
  for (const item of value.items) writer.bytes(encodeFsApplyItem(item));
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeFsApply(bytes: Uint8Array): YasFsApply {
  const cursor = new YasCursor(bytes);
  const rootHandle = cursor.u64("FS root handle");
  const operationId = new Uint8Array(cursor.take(16, "FS operation ID"));
  const flags = cursor.u16("FS APPLY flags");
  const count = cursor.u16("FS APPLY item count");
  if (
    count === 0 ||
    count > g.YAS_FS_MAX_BATCH_ITEMS ||
    count > Math.floor(cursor.remaining / 8)
  )
    throw new YasProtocolError("invalid FS APPLY item count");
  const items: YasFsApplyItem[] = [];
  for (let index = 0; index < count; index++)
    items.push(decodeFsApplyItem(cursor));
  const value = {
    rootHandle,
    operationId,
    flags,
    items,
    extensions: decodeExtensions(cursor, new Set(), "FS APPLY extensions"),
  };
  cursor.end("FS APPLY");
  encodeFsApply(value);
  return value;
}

export function encodeFsApplyResult(value: YasFsApplyResult): Uint8Array {
  requireRevision(value.rootRevision, "FS root revision");
  if (value.items.length === 0 || value.items.length > g.YAS_FS_MAX_BATCH_ITEMS)
    throw new YasProtocolError("invalid FS APPLY result count");
  const indices = new Set<number>();
  const writer = new YasWriter()
    .u64(value.rootRevision)
    .u16(value.items.length)
    .u16(0);
  for (const item of value.items) {
    if (
      indices.has(item.index) ||
      !u16(item.index) ||
      !u16(item.status) ||
      item.status > g.YAS_STATUS_INTERNAL ||
      (item.status === g.YAS_STATUS_OK && item.entryRevision === 0n) ||
      utf8Length(item.detail) > 4096
    )
      throw new YasProtocolError("invalid FS APPLY item Result");
    indices.add(item.index);
    writer
      .u16(item.index)
      .u16(item.status)
      .u64(item.entryRevision)
      .i64(item.modifiedUnixNs)
      .u8(item.contentHash ? 1 : 0)
      .bytes(new Uint8Array(3));
    if (item.contentHash) {
      requireHash(item.contentHash);
      writer.bytes(item.contentHash);
    }
    writer.utf8U16(item.detail);
  }
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeFsApplyResult(bytes: Uint8Array): YasFsApplyResult {
  const cursor = new YasCursor(bytes);
  const rootRevision = cursor.u64("FS root revision");
  const count = cursor.u16("FS APPLY result count");
  if (
    cursor.u16("FS APPLY Result reserved") !== 0 ||
    count === 0 ||
    count > g.YAS_FS_MAX_BATCH_ITEMS ||
    count > Math.floor(cursor.remaining / 26)
  )
    throw new YasProtocolError("invalid FS APPLY result count");
  const items: YasFsApplyItemResult[] = [];
  for (let index = 0; index < count; index++) {
    const itemIndex = cursor.u16("FS APPLY result index");
    const status = cursor.u16("FS APPLY result status");
    const entryRevision = cursor.u64("FS entry revision");
    const modifiedUnixNs = cursor.i64("FS modified time");
    const present = cursor.u8("FS result hash presence");
    requireZero(cursor.take(3, "FS result reserved"), "FS result");
    if (present > 1)
      throw new YasProtocolError("invalid FS result hash presence");
    items.push({
      index: itemIndex,
      status,
      entryRevision,
      modifiedUnixNs,
      contentHash: present
        ? new Uint8Array(cursor.take(32, "FS content hash"))
        : undefined,
      detail: cursor.utf8U16("FS result detail"),
    });
  }
  const value = {
    rootRevision,
    items,
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "FS APPLY Result extensions",
    ),
  };
  cursor.end("FS APPLY Result");
  encodeFsApplyResult(value);
  return value;
}

export interface YasFsStagingUpload {
  stagingHandle: bigint;
  transfer: YasTransfer;
  extensions: readonly YasExtension[];
}

export class YasFsCatalog {
  private current = new Map<string, YasFsEntryRecord>();
  private staging: Map<string, YasFsEntryRecord> | null = null;
  private retention: YasStateCatalogueRetention<string>;
  private stagingRetention: YasStateCatalogueRetention<string> | null = null;
  private subscription: YasStateSubscription | null = null;
  private revision = 0n;
  private listeners = new Set<(snapshot: YasFsSnapshot) => void>();
  private batchListeners = new Set<(batch: YasStateBatch) => void>();
  private snapshotRejectors = new Set<(error: Error) => void>();
  private removeInvalidation: (() => void) | null;
  private watchPromise: Promise<void> | null = null;
  private cancelPendingWatch: ((error: Error) => void) | null = null;
  private generation = 0;
  private disposed = false;

  constructor(
    private readonly connection: YasConnection,
    readonly rootHandle: bigint,
  ) {
    this.retention = YasStateCatalogueRetention.forConnection(connection);
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family === undefined || family === g.YAS_FAMILY_FS)
        this.invalidateLocal();
    });
  }

  get snapshot(): YasFsSnapshot {
    return { revision: this.revision, entries: [...this.current.values()] };
  }

  subscribe(listener: (snapshot: YasFsSnapshot) => void): () => void {
    this.assertOpen();
    this.listeners.add(listener);
    invokeLifecycleListener(listener, this.snapshot, "FS catalogue");
    return () => this.listeners.delete(listener);
  }

  /**
   * Observe validated state batches after they have been applied to the
   * catalogue. This is intentionally lower-level than {@link subscribe}:
   * incremental native consumers need snapshot phase boundaries and
   * first-class MOVE records, which cannot be recovered from successive whole
   * snapshots.
   */
  subscribeBatches(listener: (batch: YasStateBatch) => void): () => void {
    this.assertOpen();
    this.batchListeners.add(listener);
    return () => this.batchListeners.delete(listener);
  }

  async firstSnapshot(
    options: YasWatchOptions & {
      flags?: number;
      settleMs?: number;
      inlineMax?: number;
      ignorePatterns?: string;
    } = {},
  ): Promise<YasFsSnapshot> {
    this.assertOpen();
    if (this.revision !== 0n && this.subscription?.active) return this.snapshot;
    let remove: (() => void) | undefined;
    let rejectLifecycle!: (error: Error) => void;
    const result = new Promise<YasFsSnapshot>((resolve) => {
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
    options: YasWatchOptions & {
      flags?: number;
      settleMs?: number;
      inlineMax?: number;
      ignorePatterns?: string;
    } = {},
  ): Promise<void> {
    this.assertOpen();
    if (this.subscription?.active) return;
    if (this.watchPromise) return this.watchPromise;
    this.clearState();
    const generation = this.generation;
    const flags = options.flags ?? g.YAS_FS_WATCH_RECURSIVE;
    const settleMs = options.settleMs ?? 0;
    const inlineMax = options.inlineMax ?? g.YAS_FS_MAX_INLINE_BYTES;
    const ignorePatterns = options.ignorePatterns ?? "";
    const operation = YasStateSubscription.watch(
      this.connection,
      g.YAS_FAMILY_FS,
      g.YAS_FS_WATCH,
      g.YAS_FS_UNWATCH,
      g.YAS_FS_STATE,
      g.YAS_FS_STATE_ACK,
      options,
      (batch) => this.apply(batch),
      {
        knownRecordKinds: new Set([
          YAS_STATE_ADD,
          YAS_STATE_REPLACE,
          YAS_STATE_PATCH,
          YAS_STATE_REMOVE,
          g.YAS_FS_RECORD_MOVE,
        ]),
      },
      (statePayload) =>
        encodeFsWatch(
          this.rootHandle,
          flags,
          settleMs,
          inlineMax,
          ignorePatterns,
          statePayload,
        ),
    ).then(async (subscription) => {
      if (this.disposed || generation !== this.generation) {
        await subscription.unwatch().catch(() => undefined);
        throw new YasProtocolError(
          "FS catalogue changed while WATCH was pending",
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
    this.cancelWatch("FS catalogue WATCH was cancelled");
    this.cancelSnapshots("FS catalogue snapshot wait was cancelled");
    this.generation++;
    const subscription = this.subscription;
    this.subscription = null;
    this.clearState();
    await subscription?.unwatch();
  }

  async dispose(): Promise<void> {
    if (this.disposed) return;
    this.disposed = true;
    this.cancelWatch("FS catalogue was closed while WATCH was pending");
    this.cancelSnapshots("FS catalogue closed before its first snapshot");
    this.generation++;
    this.removeInvalidation?.();
    this.removeInvalidation = null;
    const subscription = this.subscription;
    this.subscription = null;
    this.clearState();
    this.listeners.clear();
    this.batchListeners.clear();
    await subscription?.unwatch();
  }

  private apply(batch: YasStateBatch): void {
    if (this.disposed) return;
    if (batch.phase === YAS_STATE_RESET) {
      this.clearState();
      this.emitBatch(batch);
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_BEGIN) {
      this.discardStaging();
      this.staging = new Map();
      this.stagingRetention = YasStateCatalogueRetention.forConnection(
        this.connection,
      );
      this.emitBatch(batch);
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_RECORDS) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("FS snapshot records without begin");
      try {
        this.applyRecords(this.staging, this.stagingRetention, batch.records);
        this.validateCatalog(this.staging);
        this.emitBatch(batch);
      } catch (error) {
        this.discardStaging();
        throw error;
      }
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_END) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("FS snapshot end without begin");
      try {
        this.applyRecords(this.staging, this.stagingRetention, batch.records);
        this.validateCatalog(this.staging);
      } catch (error) {
        this.discardStaging();
        throw error;
      }
      const previousRetention = this.retention;
      this.current = this.staging;
      this.retention = this.stagingRetention;
      this.staging = null;
      this.stagingRetention = null;
      previousRetention.dispose();
      this.revision = batch.toRevision;
      this.emit();
      this.emitBatch(batch);
      return;
    }
    if (batch.phase === YAS_STATE_DELTA) {
      const retention = this.retention.clone();
      let next: Map<string, YasFsEntryRecord>;
      try {
        next = new Map(this.current);
        this.applyRecords(next, retention, batch.records);
        this.validateCatalog(next);
      } catch (error) {
        retention.dispose();
        throw error;
      }
      const previousRetention = this.retention;
      this.current = next;
      this.retention = retention;
      previousRetention.dispose();
      this.revision = batch.toRevision;
      this.emit();
      this.emitBatch(batch);
    }
  }

  private applyRecords(
    target: Map<string, YasFsEntryRecord>,
    retention: YasStateCatalogueRetention<string>,
    records: readonly YasTypedRecord[],
  ): void {
    for (const action of records) {
      if (action.kind === YAS_STATE_ADD || action.kind === YAS_STATE_REPLACE) {
        const entry = detachStateRetainedValue(decodeFsEntry(action.body));
        const key = pathKey(entry.path);
        const exists = target.has(key);
        if ((action.kind === YAS_STATE_ADD) === exists)
          throw new YasProtocolError("FS ADD/REPLACE precondition failed");
        if (action.kind === YAS_STATE_ADD && target.size >= this.catalogLimit())
          throw new YasProtocolError(
            "FS catalogue exceeds its negotiated entry limit",
          );
        retention.upsert(
          key,
          Math.max(
            encodeFsEntry(entry).length,
            estimateStateRetainedBytes(entry),
          ),
        );
        target.set(key, entry);
      } else if (action.kind === YAS_STATE_PATCH) {
        const patch = decodeFsEntryPatch(action.body);
        const key = pathKey(patch.path);
        const previous = target.get(key);
        if (!previous || previous.entryRevision !== patch.observedRevision)
          throw new YasProtocolError("FS PATCH precondition failed");
        const replacement = detachStateRetainedValue(patch.replacement);
        retention.upsert(
          key,
          Math.max(
            encodeFsEntry(replacement).length,
            estimateStateRetainedBytes(replacement),
          ),
        );
        target.set(key, replacement);
      } else if (action.kind === YAS_STATE_REMOVE) {
        const removed = decodeFsRemoveRecord(action.body);
        const key = pathKey(removed.path);
        if (!target.has(key))
          throw new YasProtocolError("FS REMOVE names an unknown path");
        retention.remove(key);
        target.delete(key);
      } else if (action.kind === g.YAS_FS_RECORD_MOVE) {
        const move = decodeFsMove(action.body);
        const from = pathKey(move.from);
        const to = pathKey(move.to);
        const previous = target.get(from);
        if (!previous || target.has(to))
          throw new YasProtocolError("FS MOVE precondition failed");
        const moved = detachStateRetainedValue({ ...previous, path: move.to });
        retention.move(
          from,
          to,
          Math.max(
            encodeFsEntry(moved).length,
            estimateStateRetainedBytes(moved),
          ),
        );
        target.delete(from);
        target.set(to, moved);
      } else throw new YasProtocolError("unsupported FS state record kind");
    }
  }

  private validateCatalog(
    records: ReadonlyMap<string, YasFsEntryRecord>,
  ): void {
    if (records.size > this.catalogLimit())
      throw new YasProtocolError(
        "FS catalogue exceeds its negotiated entry limit",
      );
  }

  private catalogLimit(): number {
    return negotiatedStateLimitU32(
      this.connection,
      g.YAS_FAMILY_FS,
      g.YAS_FS_VERSION,
      g.YAS_FS_LIMIT_MAX_CATALOG_ENTRIES,
      g.YAS_FS_MAX_CATALOG_ENTRIES,
    );
  }

  private invalidateLocal(): void {
    if (this.disposed) return;
    this.cancelWatch("FS catalogue was invalidated while WATCH was pending");
    this.cancelSnapshots("FS catalogue invalidated before its first snapshot");
    this.generation++;
    this.subscription = null;
    this.clearState();
  }

  private clearState(): void {
    this.retention.dispose();
    this.stagingRetention?.dispose();
    this.current = new Map();
    this.staging = null;
    this.retention = YasStateCatalogueRetention.forConnection(this.connection);
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
      invokeLifecycleListener(listener, snapshot, "FS catalogue");
  }

  private emitBatch(batch: YasStateBatch): void {
    if (this.disposed) return;
    for (const listener of this.batchListeners)
      invokeLifecycleListener(listener, batch, "FS catalogue batch");
  }

  private assertOpen(): void {
    if (this.disposed) throw new YasProtocolError("FS catalogue is closed");
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

export class YasFsClient {
  private readonly roots = new Set<YasFsRoot>();
  private removeInvalidation: (() => void) | null;
  private generation = 0;
  private disposed = false;

  constructor(readonly connection: YasConnection) {
    connection.family(g.YAS_FAMILY_FS, g.YAS_FS_VERSION);
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family !== undefined && family !== g.YAS_FAMILY_FS) return;
      this.generation++;
      for (const root of [...this.roots]) root.invalidate();
      this.roots.clear();
    });
  }

  async open(value: YasFsOpen): Promise<YasFsRoot> {
    this.assertOpen();
    const generation = this.generation;
    const opened = await this.connection.requestDecoded(
      g.YAS_FAMILY_FS,
      g.YAS_FS_OPEN,
      encodeFsOpen(value),
      decodeFsOpenResult,
    );
    const root = new YasFsRoot(this, opened);
    if (this.disposed || generation !== this.generation) {
      await root.close().catch(() => undefined);
      throw new YasProtocolError("FS client changed while OPEN was pending");
    }
    this.roots.add(root);
    return root;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.generation++;
    this.removeInvalidation?.();
    this.removeInvalidation = null;
    for (const root of [...this.roots])
      void root.close().catch(() => undefined);
    this.roots.clear();
  }

  release(root: YasFsRoot): void {
    this.roots.delete(root);
  }

  private assertOpen(): void {
    if (this.disposed) throw new YasProtocolError("FS client is closed");
  }
}

export class YasFsRoot {
  readonly catalog: YasFsCatalog;
  private readonly transfers;
  private closed = false;

  constructor(
    readonly client: YasFsClient,
    readonly opened: YasFsOpenResult,
  ) {
    this.catalog = new YasFsCatalog(client.connection, opened.rootHandle);
    this.transfers = transfersFor(client.connection);
  }

  get handle(): bigint {
    return this.opened.rootHandle;
  }

  list(
    options: YasWatchOptions & {
      flags?: number;
      settleMs?: number;
      inlineMax?: number;
      ignorePatterns?: string;
    } = {},
  ): Promise<YasFsSnapshot> {
    this.assertOpen();
    return this.catalog.firstSnapshot(options);
  }

  async close(extensions: readonly YasExtension[] = []): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.client.release(this);
    await this.catalog.dispose().catch(() => undefined);
    await this.client.connection.request(
      g.YAS_FAMILY_FS,
      g.YAS_FS_CLOSE,
      encodeFsClose(this.handle, extensions),
    );
  }

  invalidate(): void {
    if (this.closed) return;
    this.closed = true;
    this.client.release(this);
    void this.catalog.dispose().catch(() => undefined);
  }

  async fetch(
    path: YasFsPath,
    options: {
      expectedHash?: Uint8Array;
      initialReceiveCredit?: bigint;
      extensions?: readonly YasExtension[];
    } = {},
  ): Promise<YasFsContent> {
    this.assertOpen();
    const lease = this.transfers.reserveReceiveCredit(
      options.initialReceiveCredit ?? 1024n * 1024n,
      1n,
    );
    let accepted = false;
    let released = false;
    try {
      return await this.client.connection.requestDecoded(
        g.YAS_FAMILY_FS,
        g.YAS_FS_FETCH,
        encodeFsFetch({
          rootHandle: this.handle,
          path,
          expectedHash: options.expectedHash,
          initialReceiveCredit: lease.bytes,
          extensions: options.extensions,
        }),
        (body) => {
          const result = decodeFsContentResult(body).content;
          if (result.delivery === "inline") {
            lease.release();
            released = true;
            const bytes = new Uint8Array(result.bytes);
            return {
              byteLength: result.byteLength,
              contentHash: new Uint8Array(result.contentHash),
              bytes: () => Promise.resolve(new Uint8Array(bytes)),
            };
          }
          const transfer = this.transfers.acceptServerDescriptor(
            result.descriptor,
            lease,
          );
          accepted = true;
          const bytes = transfer.collect(result.byteLength);
          return {
            byteLength: result.byteLength,
            contentHash: new Uint8Array(result.contentHash),
            bytes: () => bytes.then((value) => new Uint8Array(value)),
          };
        },
      );
    } catch (error) {
      if (!accepted && !released) lease.release();
      throw error;
    }
  }

  read(
    questions: readonly YasFsReadQuestion[],
    initialReceiveCredit = 1024n * 1024n,
    extensions: readonly YasExtension[] = [],
  ): Promise<YasFsQueryPage> {
    return this.query(g.YAS_FS_READ, initialReceiveCredit, (credit) =>
      encodeFsRead({
        rootHandle: this.handle,
        initialReceiveCredit: credit,
        questions,
        extensions,
      }),
    );
  }

  search(
    value: Omit<YasFsSearch, "rootHandle" | "initialReceiveCredit">,
    initialReceiveCredit = 1024n * 1024n,
  ): Promise<YasFsQueryPage> {
    return this.query(g.YAS_FS_SEARCH, initialReceiveCredit, (credit) =>
      encodeFsSearch({
        ...value,
        rootHandle: this.handle,
        initialReceiveCredit: credit,
      }),
    );
  }

  index(
    value: Omit<YasFsIndex, "rootHandle" | "initialReceiveCredit">,
    initialReceiveCredit = 1024n * 1024n,
  ): Promise<YasFsQueryPage> {
    return this.query(g.YAS_FS_INDEX, initialReceiveCredit, (credit) =>
      encodeFsIndex({
        ...value,
        rootHandle: this.handle,
        initialReceiveCredit: credit,
      }),
    );
  }

  grep(
    value: Omit<YasFsGrep, "rootHandle" | "initialReceiveCredit">,
    initialReceiveCredit = 1024n * 1024n,
  ): Promise<YasFsQueryPage> {
    return this.query(g.YAS_FS_GREP, initialReceiveCredit, (credit) =>
      encodeFsGrep({
        ...value,
        rootHandle: this.handle,
        initialReceiveCredit: credit,
      }),
    );
  }

  async stageWrite(
    value: Omit<YasFsStageWrite, "rootHandle">,
  ): Promise<YasFsStagingUpload> {
    this.assertOpen();
    const result = await this.client.connection.requestDecoded(
      g.YAS_FAMILY_FS,
      g.YAS_FS_STAGE_WRITE,
      encodeFsStageWrite({ ...value, rootHandle: this.handle }),
      decodeFsStageWriteResult,
    );
    return {
      stagingHandle: result.stagingHandle,
      transfer: this.transfers.acceptServerUploadDescriptor(result.descriptor),
      extensions: result.extensions,
    };
  }

  commit(
    stagingHandle: bigint,
    operationId: Uint8Array,
    flags = 0,
    extensions: readonly YasExtension[] = [],
  ): Promise<YasFsCommitResult> {
    this.assertOpen();
    return this.client.connection.requestDecoded(
      g.YAS_FAMILY_FS,
      g.YAS_FS_COMMIT,
      encodeFsCommit({ stagingHandle, operationId, flags, extensions }),
      decodeFsCommitResult,
    );
  }

  apply(value: Omit<YasFsApply, "rootHandle">): Promise<YasFsApplyResult> {
    this.assertOpen();
    return this.client.connection.requestDecoded(
      g.YAS_FAMILY_FS,
      g.YAS_FS_APPLY,
      encodeFsApply({ ...value, rootHandle: this.handle }),
      decodeFsApplyResult,
    );
  }

  private async query(
    kind: number,
    initialReceiveCredit: bigint,
    payload: (credit: bigint) => Uint8Array,
  ): Promise<YasFsQueryPage> {
    this.assertOpen();
    const lease = this.transfers.reserveReceiveCredit(initialReceiveCredit, 1n);
    let accepted = false;
    let released = false;
    try {
      return await this.client.connection.requestDecoded(
        g.YAS_FAMILY_FS,
        kind,
        payload(lease.bytes),
        (body) => {
          const result = decodeFsQueryPage(body);
          if (result.delivery.kind === "inline") {
            lease.release();
            released = true;
            const records = result.delivery.records.map(cloneFsTypedRecord);
            return {
              nextCursor: new Uint8Array(result.nextCursor),
              totalHint: result.totalHint,
              flags: result.flags,
              records: () => Promise.resolve(records.map(cloneFsTypedRecord)),
            };
          }
          const transfer = this.transfers.acceptServerDescriptor(
            result.delivery.descriptor,
            lease,
          );
          accepted = true;
          const records = collectFsQueryRecords(transfer);
          return {
            nextCursor: new Uint8Array(result.nextCursor),
            totalHint: result.totalHint,
            flags: result.flags,
            records: () =>
              records.then((value) => value.map(cloneFsTypedRecord)),
          };
        },
      );
    } catch (error) {
      if (!accepted && !released) lease.release();
      throw error;
    }
  }

  private assertOpen(): void {
    if (this.closed) throw new YasProtocolError("FS root is closed");
  }
}

async function collectFsQueryRecords(
  transfer: YasTransfer,
): Promise<readonly YasFsTypedRecord[]> {
  const records: YasFsTypedRecord[] = [];
  let byteLength = 0;
  let nextRecordIndex = 0;
  try {
    while (true) {
      const message = await transfer.readMessage();
      if (message === null) break;
      byteLength += message.length;
      if (byteLength > g.YAS_FS_MAX_QUERY_BYTES)
        throw new YasProtocolError("FS query Transfer exceeds its byte limit");
      const batch = decodeFsQueryRecordBatch(message);
      if (batch.firstRecordIndex !== nextRecordIndex)
        throw new YasProtocolError("FS query batch sequence is discontinuous");
      records.push(...batch.records);
      nextRecordIndex += batch.records.length;
      if (records.length > g.YAS_FS_MAX_QUERY_RECORDS)
        throw new YasProtocolError(
          "FS query Transfer exceeds its record limit",
        );
    }
    validateFsQueryRecords(records, true);
    return records;
  } catch (error) {
    transfer.reset();
    throw error;
  }
}

function cloneFsTypedRecord(record: YasFsTypedRecord): YasFsTypedRecord {
  return {
    kind: record.kind,
    required: record.required,
    body: new Uint8Array(record.body),
  };
}

function validatePath(value: YasFsPath): void {
  if (value.components.length > g.YAS_FS_MAX_PATH_COMPONENTS)
    throw new YasProtocolError("too many FS path components");
  let total = 0;
  for (const component of value.components) {
    if (
      component.length === 0 ||
      component.length > g.YAS_FS_MAX_COMPONENT_BYTES ||
      (component.length === 1 && component[0] === 0x2e) ||
      (component.length === 2 &&
        component[0] === 0x2e &&
        component[1] === 0x2e) ||
      component.includes(0) ||
      component.includes(0x2f) ||
      component.includes(0x5c)
    )
      throw new YasProtocolError("invalid FS path component");
    total += component.length;
    if (total > g.YAS_FS_MAX_PATH_BYTES)
      throw new YasProtocolError("FS path exceeds its byte limit");
  }
}

function validateRootSource(value: YasFsRootSource): void {
  if (value.kind === "platform-path") {
    if (
      value.path.length === 0 ||
      value.path.length > g.YAS_FS_MAX_PATH_BYTES ||
      value.path.includes(0)
    )
      throw new YasProtocolError("invalid FS platform path");
  } else if (value.kind === "terminal-cwd") {
    requireHandle(value.terminalHandle, "FS terminal handle");
    validatePath(value.suffix);
  } else if (value.kind === "process-cwd")
    requireHandle(value.processHandle, "FS process handle");
}

function validateOpenResult(value: YasFsOpenResult): void {
  requireHandle(value.rootHandle, "FS root handle");
  requireRevision(value.rootRevision, "FS root revision");
  if (
    value.pathModel < g.YAS_FS_PATH_POSIX_BYTES ||
    value.pathModel > g.YAS_FS_PATH_WINDOWS_UTF8 ||
    value.caseBehavior < g.YAS_FS_CASE_SENSITIVE ||
    value.caseBehavior > g.YAS_FS_CASE_PRESERVING_INSENSITIVE ||
    value.canonicalPath.length === 0 ||
    value.canonicalPath.length > g.YAS_FS_MAX_PATH_BYTES ||
    value.canonicalPath.includes(0)
  )
    throw new YasProtocolError("invalid FS root path metadata");
}

function validateEntry(value: YasFsEntryRecord): void {
  validatePath(value.path);
  requireRevision(value.entryRevision, "FS entry revision");
  if (value.flags & ~g.YAS_FS_ENTRY_FLAGS)
    throw new YasProtocolError("invalid FS entry flags");
  const unreadable = Boolean(value.flags & g.YAS_FS_ENTRY_UNREADABLE);
  const unstable = Boolean(value.flags & g.YAS_FS_ENTRY_UNSTABLE);
  const symlinkDirectory = Boolean(
    value.flags & g.YAS_FS_ENTRY_SYMLINK_DIRECTORY,
  );
  const directoryFiltered = Boolean(
    value.flags & g.YAS_FS_ENTRY_DIRECTORY_FILTERED,
  );
  if (value.body.kind === "file") {
    requireHash(value.body.contentHash);
    if (
      value.body.inlineContent !== undefined &&
      (value.body.inlineContent.length > g.YAS_FS_MAX_INLINE_BYTES ||
        BigInt(value.body.inlineContent.length) !== value.body.byteLength)
    )
      throw new YasProtocolError("invalid FS inline file content");
    if ((unreadable || unstable) && value.body.inlineContent !== undefined)
      throw new YasProtocolError("unavailable FS file has inline content");
    if (symlinkDirectory || directoryFiltered)
      throw new YasProtocolError("invalid FS file-only flags");
  } else if (value.body.kind === "directory") {
    if (unreadable || unstable || symlinkDirectory)
      throw new YasProtocolError("invalid FS directory flags");
  } else {
    requireHash(value.body.contentHash);
    if (
      value.body.target.length === 0 ||
      value.body.target.length > g.YAS_FS_MAX_PATH_BYTES ||
      value.body.target.includes(0)
    )
      throw new YasProtocolError("invalid FS symlink target");
    if (unstable || (directoryFiltered && !symlinkDirectory))
      throw new YasProtocolError("invalid FS symlink flags");
  }
  const operation = value.extensions.find(
    (extension) => extension.tag === g.YAS_FS_ENTRY_OPERATION_ID_EXTENSION,
  );
  if (operation) requireOperationId(operation.value);
}

function validateReadQuestion(value: YasFsReadQuestion): void {
  if (
    value.kind < g.YAS_FS_READ_STAT ||
    value.kind > g.YAS_FS_READ_CONTENT ||
    value.flags & ~g.YAS_FS_READ_FLAGS
  )
    throw new YasProtocolError("invalid FS READ question");
  validatePath(value.path);
}

function validateQuery(
  rootHandle: bigint,
  flags: number,
  allowedFlags: number,
  query: Uint8Array,
  cursor: Uint8Array,
): void {
  requireHandle(rootHandle, "FS root handle");
  if (
    flags & ~allowedFlags ||
    query.length === 0 ||
    query.length > g.YAS_FS_MAX_QUERY_TEXT_BYTES ||
    query.includes(0) ||
    cursor.length > g.YAS_FS_MAX_CURSOR_BYTES
  )
    throw new YasProtocolError("invalid FS query request");
}

function validateQueryPage(value: YasFsQueryPageWire): void {
  if (
    value.nextCursor.length > g.YAS_FS_MAX_CURSOR_BYTES ||
    value.flags & ~g.YAS_FS_PAGE_FLAGS
  )
    throw new YasProtocolError("invalid FS query cursor");
  if (value.delivery.kind === "inline") {
    if (value.delivery.records.length > g.YAS_FS_MAX_QUERY_RECORDS)
      throw new YasProtocolError("too many inline FS query records");
    const encoded = concat(value.delivery.records.map(encodeFsTypedRecord));
    if (encoded.length > g.YAS_FS_MAX_QUERY_BYTES)
      throw new YasProtocolError("inline FS query bytes exceed their limit");
    validateFsQueryRecords(value.delivery.records, true);
  } else
    validateFsTransfer(
      value.delivery.descriptor,
      g.YAS_FS_QUERY_CONTENT_KIND,
      YAS_TRANSFER_MODE_MESSAGE,
      YAS_TRANSFER_SENDER_TO_RECEIVER,
    );
}

function validateFsQueryRecords(
  records: readonly YasFsTypedRecord[],
  completePage: boolean,
): void {
  let category: "read" | "path" | "grep" | undefined;
  const expectedMatches: number[] = [];
  const actualMatches: number[] = [];
  for (const record of records) {
    const decoded = decodeFsQueryRecord(record);
    if (decoded.kind === "unknown") continue;
    const current =
      decoded.kind === "read"
        ? "read"
        : decoded.kind === "path"
          ? "path"
          : "grep";
    if (category !== undefined && category !== current)
      throw new YasProtocolError("mixed FS query record categories");
    category = current;
    if (decoded.kind === "grep-file") {
      if (decoded.value.fileIndex !== expectedMatches.length)
        throw new YasProtocolError("FS GREP file index is out of order");
      expectedMatches.push(decoded.value.matchCount);
      actualMatches.push(0);
    } else if (decoded.kind === "grep-match") {
      const index = decoded.value.fileIndex;
      if (index >= actualMatches.length)
        throw new YasProtocolError("FS GREP match names an unknown file");
      actualMatches[index] = actualMatches[index]! + 1;
    }
  }
  if (
    completePage &&
    (expectedMatches.length !== actualMatches.length ||
      expectedMatches.some(
        (expected, index) => actualMatches[index] !== expected,
      ))
  )
    throw new YasProtocolError("FS GREP match count mismatch");
}

function validateFsContentDelivery(value: YasInlineOrTransfer): void {
  requireHash(value.contentHash);
  if (value.delivery === "inline") {
    if (value.bytes.length > g.YAS_FS_MAX_INLINE_BYTES)
      throw new YasProtocolError("inline FS content exceeds its limit");
  } else
    validateFsTransfer(
      value.descriptor,
      g.YAS_FS_FILE_CONTENT_KIND,
      YAS_TRANSFER_MODE_BYTE,
      YAS_TRANSFER_SENDER_TO_RECEIVER,
    );
}

function validateFsTransfer(
  descriptor: YasTransferDescriptor,
  contentKind: number,
  mode: number,
  direction: number,
): void {
  encodeTransferDescriptor(descriptor);
  if (
    descriptor.mode !== mode ||
    descriptor.direction !== direction ||
    descriptor.contentFamily !== g.YAS_FAMILY_FS ||
    descriptor.contentKind !== contentKind ||
    descriptor.contentVersion !== g.YAS_FS_VERSION ||
    descriptor.sensitiveContent !== true
  )
    throw new YasProtocolError("invalid FS Transfer descriptor");
}

function decodeDescriptor(bytes: Uint8Array): YasTransferDescriptor {
  const cursor = new YasCursor(bytes);
  const value = decodeTransferDescriptor(cursor);
  cursor.end("FS Transfer descriptor");
  return value;
}

function pathKey(value: YasFsPath): string {
  return Array.from(encodeFsPath(value), (byte) =>
    byte.toString(16).padStart(2, "0"),
  ).join("");
}

function samePath(left: YasFsPath, right: YasFsPath): boolean {
  if (left.components.length !== right.components.length) return false;
  return left.components.every((component, index) => {
    const other = right.components[index]!;
    return (
      component.length === other.length &&
      component.every((byte, byteIndex) => byte === other[byteIndex])
    );
  });
}

function requireHandle(value: bigint, field: string): void {
  if (value === 0n) throw new YasProtocolError(`${field} is zero`);
}

function requireRevision(value: bigint, field: string): void {
  if (value === 0n) throw new YasProtocolError(`${field} is zero`);
}

function requireOperationId(value: Uint8Array): void {
  if (value.length !== 16 || value.every((byte) => byte === 0))
    throw new YasProtocolError("invalid FS operation ID");
}

function requireHash(value: Uint8Array): void {
  if (value.length !== 32)
    throw new YasProtocolError("FS content hash is not 32 bytes");
}

function requireZero(value: Uint8Array, field: string): void {
  if (value.some((byte) => byte !== 0))
    throw new YasProtocolError(`${field} reserved bytes are nonzero`);
}

function concat(parts: readonly Uint8Array[]): Uint8Array {
  const output = new Uint8Array(
    parts.reduce((length, part) => length + part.length, 0),
  );
  let offset = 0;
  for (const part of parts) {
    output.set(part, offset);
    offset += part.length;
  }
  return output;
}

function utf8Length(value: string): number {
  return new TextEncoder().encode(value).length;
}

function u16(value: number): boolean {
  return Number.isInteger(value) && value >= 0 && value <= 0xffff;
}

function u32(value: number): boolean {
  return Number.isInteger(value) && value >= 0 && value <= 0xffffffff;
}
