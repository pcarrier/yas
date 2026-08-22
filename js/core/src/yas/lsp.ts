/** YAS LSP family v1 codecs and browser client. */

import * as g from "./generated";
import type { YasConnection } from "./session";
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
  decodeTransferDescriptor,
  encodeTransferDescriptor,
  requireTransferUploadStage,
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
  YAS_FAMILY_LSP,
  YAS_LSP_BUFFER_BEGIN,
  YAS_LSP_BUFFER_CLOSE,
  YAS_LSP_BUFFER_COMMIT,
  YAS_LSP_BUFFER_PUT,
  YAS_LSP_CLOSE,
  YAS_LSP_LIST_SERVERS,
  YAS_LSP_OPEN,
  YAS_LSP_QUERY,
  YAS_LSP_STATE,
  YAS_LSP_STATE_ACK,
  YAS_LSP_STOP_SERVER,
  YAS_LSP_UNWATCH,
  YAS_LSP_VERSION,
  YAS_LSP_WATCH,
} from "./generated";

export interface YasLspPosition {
  line: number;
  byteColumn: number;
}

export interface YasLspTextRange {
  start: YasLspPosition;
  end: YasLspPosition;
}

export interface YasLspDocumentTarget {
  path: YasFsPath;
  documentRevision: bigint;
  contentHash: Uint8Array;
}

export type YasLspWorkspaceSource =
  | { kind: "fs"; rootHandle: bigint; rootPath: YasFsPath }
  | { kind: "platform-path"; path: Uint8Array }
  | { kind: "terminal-cwd"; terminalHandle: bigint; suffix: YasFsPath };

export interface YasLspOpen {
  source: YasLspWorkspaceSource;
  openMode: number;
  diagnosticsSettleMs: number;
  language: string;
  profile: string;
  initializationOptions: Uint8Array;
  extensions?: readonly YasExtension[];
}

export interface YasLspOpenResult {
  workspaceHandle: bigint;
  workspaceRevision: bigint;
  positionEncoding: number;
  backendCount: number;
  capabilities: bigint;
  canonicalRoot: Uint8Array;
  extensions: readonly YasExtension[];
}

export interface YasLspClosed {
  workspaceHandle: bigint;
  reason: number;
  detail: string;
}

export type YasLspQueryBody =
  | {
      kind: "definition";
      target: YasLspDocumentTarget;
      position: YasLspPosition;
    }
  | {
      kind: "references";
      target: YasLspDocumentTarget;
      position: YasLspPosition;
      flags: number;
    }
  | {
      kind: "hover";
      target: YasLspDocumentTarget;
      position: YasLspPosition;
    }
  | { kind: "document-symbols"; target: YasLspDocumentTarget }
  | { kind: "workspace-symbols"; query: string }
  | {
      kind: "completion";
      target: YasLspDocumentTarget;
      position: YasLspPosition;
      triggerKind: number;
      trigger: string;
    }
  | {
      kind: "code-actions";
      target: YasLspDocumentTarget;
      range: YasLspTextRange;
      diagnosticIds: readonly bigint[];
    }
  | {
      kind: "formatting";
      target: YasLspDocumentTarget;
      range?: YasLspTextRange;
      tabWidth: number;
      flags: number;
    }
  | {
      kind: "rename";
      target: YasLspDocumentTarget;
      position: YasLspPosition;
      newName: string;
    }
  | {
      kind: "signature-help";
      target: YasLspDocumentTarget;
      position: YasLspPosition;
    };

export interface YasLspQuery {
  workspaceHandle: bigint;
  maxRecords: number;
  cursor: Uint8Array;
  initialReceiveCredit: bigint;
  body: YasLspQueryBody;
  extensions?: readonly YasExtension[];
}

export interface YasLspBufferIdentity {
  bufferHandle: bigint;
  bufferRevision: bigint;
  workspaceRevision: bigint;
  byteLength: bigint;
  contentHash: Uint8Array;
  extensions: readonly YasExtension[];
}

export interface YasLspBufferPut {
  workspaceHandle: bigint;
  operationId: Uint8Array;
  expectedRevision: bigint;
  path: YasFsPath;
  content: Uint8Array;
  extensions?: readonly YasExtension[];
}

export interface YasLspBufferBegin {
  workspaceHandle: bigint;
  expectedRevision: bigint;
  path: YasFsPath;
  byteLength: bigint;
  contentHash: Uint8Array;
  initialSendCredit: bigint;
  extensions?: readonly YasExtension[];
}

export interface YasLspBufferBeginResult {
  stagingHandle: bigint;
  descriptor: YasTransferDescriptor;
  extensions: readonly YasExtension[];
}

export interface YasLspBufferCommit {
  stagingHandle: bigint;
  operationId: Uint8Array;
  extensions?: readonly YasExtension[];
}

export interface YasLspBufferClose {
  bufferHandle: bigint;
  expectedRevision: bigint;
  operationId: Uint8Array;
  extensions?: readonly YasExtension[];
}

export interface YasLspStopServer {
  serverHandle: bigint;
  generation: bigint;
  operationId: Uint8Array;
  extensions?: readonly YasExtension[];
}

export interface YasLspLocationRecord {
  kind: "location";
  path: YasFsPath;
  documentRevision: bigint;
  contentHash: Uint8Array;
  range: YasLspTextRange;
  flags: number;
}

export interface YasLspHoverRecord {
  kind: "hover";
  target: YasLspLocationRecord;
  markupKind: number;
  content: Uint8Array;
}

export interface YasLspSymbolRecord {
  kind: "symbol";
  symbolKind: number;
  flags: number;
  depth: number;
  name: string;
  detail: string;
  path?: YasFsPath;
  contentHash?: Uint8Array;
  range: YasLspTextRange;
  selectionRange: YasLspTextRange;
}

export interface YasLspCompletionRecord {
  kind: "completion";
  itemKind: number;
  flags: number;
  label: string;
  detail: string;
  filterText: string;
  insertText: Uint8Array;
  replacementRange?: YasLspTextRange;
}

export interface YasLspEditRecord {
  kind: "edit";
  path: YasFsPath;
  expectedRevision: bigint;
  expectedContentHash: Uint8Array;
  range: YasLspTextRange;
  replacement: Uint8Array;
}

export interface YasLspActionRecord {
  kind: "action";
  title: string;
  actionKind: string;
  flags: number;
  edits: readonly YasLspEditRecord[];
  disabledReason: string;
}

export interface YasLspSignatureRecord {
  kind: "signature";
  flags: number;
  activeParameter: number;
  parameterStart: number;
  parameterEnd: number;
  label: string;
  documentation: string;
}

export type YasLspQueryRecord =
  | YasLspLocationRecord
  | YasLspHoverRecord
  | YasLspSymbolRecord
  | YasLspCompletionRecord
  | YasLspActionRecord
  | YasLspEditRecord
  | YasLspSignatureRecord;

export type YasLspPageDelivery =
  | { kind: "inline"; records: readonly YasLspQueryRecord[] }
  | { kind: "transfer"; descriptor: YasTransferDescriptor };

export interface YasLspQueryPageWire {
  queryStatus: number;
  flags: number;
  detail: string;
  nextCursor: Uint8Array;
  totalHint: bigint;
  delivery: YasLspPageDelivery;
  extensions: readonly YasExtension[];
}

export interface YasLspQueryPage {
  queryStatus: number;
  flags: number;
  detail: string;
  nextCursor: Uint8Array;
  totalHint: bigint;
  records(): Promise<readonly YasLspQueryRecord[]>;
}

export interface YasLspServerRecord {
  serverHandle: bigint;
  generation: bigint;
  serverRevision: bigint;
  workspaceHandle: bigint;
  phase: number;
  progressPercent: number;
  epoch: number;
  refusedEdits: number;
  rssBytes: bigint;
  capabilities: bigint;
  language: string;
  profile: string;
  backendId: string;
  lastMessage: string;
  extensions: readonly YasExtension[];
}

export interface YasLspDiagnostic {
  diagnosticId: bigint;
  severity: number;
  tags: number;
  range: YasLspTextRange;
  code: string;
  source: string;
  message: string;
}

export interface YasLspDiagnosticRecord {
  path: YasFsPath;
  documentRevision: bigint;
  contentHash: Uint8Array;
  diagnosticsRevision: bigint;
  diagnostics: readonly YasLspDiagnostic[];
  extensions: readonly YasExtension[];
}

export interface YasLspBufferRecord {
  workspaceHandle: bigint;
  bufferHandle: bigint;
  bufferRevision: bigint;
  path: YasFsPath;
  byteLength: bigint;
  contentHash: Uint8Array;
  extensions: readonly YasExtension[];
}

export type YasLspStateEntity =
  | { kind: "backend"; value: YasLspServerRecord }
  | { kind: "diagnostics"; value: YasLspDiagnosticRecord }
  | { kind: "buffer"; value: YasLspBufferRecord };

export interface YasLspEntityPatch {
  entityKind: number;
  observedRevision: bigint;
  replacement: YasLspStateEntity;
  extensions: readonly YasExtension[];
}

export interface YasLspRemovedEntity {
  entityKind: number;
  key: Uint8Array;
  removedRevision: bigint;
}

export interface YasLspSnapshot {
  revision: bigint;
  backends: readonly YasLspServerRecord[];
  diagnostics: readonly YasLspDiagnosticRecord[];
  buffers: readonly YasLspBufferRecord[];
}

const encoder = new TextEncoder();

export function encodeLspPosition(value: YasLspPosition): Uint8Array {
  validatePosition(value);
  return new YasWriter().u32(value.line).u32(value.byteColumn).finish();
}

export function decodeLspPosition(bytes: Uint8Array): YasLspPosition {
  const cursor = new YasCursor(bytes);
  const value = decodePosition(cursor);
  cursor.end("LSP position");
  return value;
}

export function encodeLspTextRange(value: YasLspTextRange): Uint8Array {
  validateRange(value);
  return new YasWriter()
    .bytes(encodeLspPosition(value.start))
    .bytes(encodeLspPosition(value.end))
    .finish();
}

export function decodeLspTextRange(bytes: Uint8Array): YasLspTextRange {
  const cursor = new YasCursor(bytes);
  const value = decodeRange(cursor);
  cursor.end("LSP text range");
  return value;
}

export function encodeLspDocumentTarget(
  value: YasLspDocumentTarget,
): Uint8Array {
  requireDocumentPath(value.path);
  requireHash(value.contentHash);
  if (
    value.documentRevision !== 0n &&
    value.contentHash.every((byte) => byte === 0)
  )
    throw new YasProtocolError(
      "a versioned LSP document target requires an exact content hash",
    );
  return new YasWriter()
    .bytesU32(encodeFsPath(value.path))
    .u64(value.documentRevision)
    .bytes(value.contentHash)
    .finish();
}

export function encodeLspWorkspaceSource(
  value: YasLspWorkspaceSource,
): Uint8Array {
  const writer = new YasWriter();
  if (value.kind === "fs") {
    requireHandle(value.rootHandle, "LSP FS root handle");
    writer
      .u8(g.YAS_LSP_SOURCE_FS)
      .bytes(new Uint8Array(3))
      .u64(value.rootHandle)
      .bytesU32(encodeFsPath(value.rootPath));
  } else if (value.kind === "platform-path") {
    validatePlatformPath(value.path, "LSP platform root path");
    writer
      .u8(g.YAS_LSP_SOURCE_PLATFORM_PATH)
      .bytes(new Uint8Array(3))
      .bytesU32(value.path);
  } else {
    requireHandle(value.terminalHandle, "LSP Terminal handle");
    writer
      .u8(g.YAS_LSP_SOURCE_TERMINAL_CWD)
      .bytes(new Uint8Array(3))
      .u64(value.terminalHandle)
      .bytesU32(encodeFsPath(value.suffix));
  }
  return writer.finish();
}

export function decodeLspWorkspaceSource(
  bytes: Uint8Array,
): YasLspWorkspaceSource {
  const cursor = new YasCursor(bytes);
  const kind = cursor.u8("LSP workspace source kind");
  requireZero(
    cursor.take(3, "LSP workspace source reserved"),
    "LSP workspace source",
  );
  let value: YasLspWorkspaceSource;
  if (kind === g.YAS_LSP_SOURCE_FS)
    value = {
      kind: "fs",
      rootHandle: cursor.u64("LSP FS root handle"),
      rootPath: decodeFsPath(cursor.bytesU32("LSP root path")),
    };
  else if (kind === g.YAS_LSP_SOURCE_PLATFORM_PATH)
    value = {
      kind: "platform-path",
      path: new Uint8Array(cursor.bytesU32("LSP platform root path")),
    };
  else if (kind === g.YAS_LSP_SOURCE_TERMINAL_CWD)
    value = {
      kind: "terminal-cwd",
      terminalHandle: cursor.u64("LSP Terminal handle"),
      suffix: decodeFsPath(cursor.bytesU32("LSP Terminal CWD suffix")),
    };
  else throw new YasProtocolError("unknown LSP workspace source kind");
  cursor.end("LSP workspace source");
  encodeLspWorkspaceSource(value);
  return value;
}

export function decodeLspDocumentTarget(
  bytes: Uint8Array,
): YasLspDocumentTarget {
  const cursor = new YasCursor(bytes);
  const value = decodeDocumentTarget(cursor);
  cursor.end("LSP document target");
  return value;
}

export function encodeLspOpen(value: YasLspOpen): Uint8Array {
  const explicit = value.openMode === g.YAS_LSP_OPEN_EXPLICIT;
  const auto = value.openMode === g.YAS_LSP_OPEN_AUTO_DISCOVER;
  if (
    (!explicit && !auto) ||
    value.diagnosticsSettleMs > g.YAS_LSP_MAX_DIAGNOSTICS_SETTLE_MS ||
    value.initializationOptions.length > g.YAS_LSP_MAX_INITIALIZATION_BYTES
  )
    throw new YasProtocolError("LSP initialization options exceed limit");
  if (explicit) {
    requireName(value.language, g.YAS_LSP_MAX_LANGUAGE_BYTES, "LSP language");
    requireName(value.profile, g.YAS_LSP_MAX_PROFILE_BYTES, "LSP profile");
  } else if (
    value.language.length !== 0 ||
    value.profile.length !== 0 ||
    value.initializationOptions.length !== 0
  )
    throw new YasProtocolError("invalid LSP auto-discovery metadata");
  rejectRequiredExtensions(value.extensions, "LSP OPEN");
  return new YasWriter()
    .bytesU32(encodeLspWorkspaceSource(value.source))
    .u8(value.openMode)
    .u8(0)
    .u16(value.diagnosticsSettleMs)
    .utf8U16(value.language)
    .utf8U16(value.profile)
    .bytesU32(value.initializationOptions)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeLspOpen(bytes: Uint8Array): YasLspOpen {
  const cursor = new YasCursor(bytes);
  const source = decodeLspWorkspaceSource(
    cursor.bytesU32("LSP workspace source"),
  );
  const openMode = cursor.u8("LSP OPEN mode");
  if (cursor.u8("LSP OPEN reserved") !== 0)
    throw new YasProtocolError("LSP OPEN reserved byte is nonzero");
  const value = {
    source,
    openMode,
    diagnosticsSettleMs: cursor.u16("LSP diagnostics settle time"),
    language: cursor.utf8U16("LSP language"),
    profile: cursor.utf8U16("LSP profile"),
    initializationOptions: new Uint8Array(
      cursor.bytesU32("LSP initialization options"),
    ),
    extensions: decodeExtensions(cursor, new Set(), "LSP OPEN extensions"),
  };
  cursor.end("LSP OPEN");
  encodeLspOpen(value);
  return value;
}

export function encodeLspOpenResult(value: YasLspOpenResult): Uint8Array {
  requireHandle(value.workspaceHandle, "LSP workspace handle");
  requireRevision(value.workspaceRevision, "LSP workspace revision");
  const noBackendDetail = decodeLspNoBackendDetail(value.extensions);
  if (
    value.positionEncoding !== g.YAS_LSP_POSITION_UTF8 ||
    value.backendCount > g.YAS_LSP_MAX_SERVERS ||
    value.capabilities & ~BigInt(g.YAS_LSP_CAPABILITIES) ||
    (value.backendCount === 0) !== (noBackendDetail !== undefined)
  )
    throw new YasProtocolError("invalid LSP workspace metadata");
  validatePlatformPath(value.canonicalRoot, "LSP canonical root");
  return new YasWriter()
    .u64(value.workspaceHandle)
    .u64(value.workspaceRevision)
    .u8(value.positionEncoding)
    .u8(0)
    .u16(value.backendCount)
    .u64(value.capabilities)
    .bytesU32(value.canonicalRoot)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeLspOpenResult(bytes: Uint8Array): YasLspOpenResult {
  const cursor = new YasCursor(bytes);
  const value = {
    workspaceHandle: cursor.u64("LSP workspace handle"),
    workspaceRevision: cursor.u64("LSP workspace revision"),
    positionEncoding: cursor.u8("LSP position encoding"),
    backendCount: 0,
    capabilities: 0n,
    canonicalRoot: new Uint8Array(),
    extensions: [] as YasExtension[],
  };
  if (cursor.u8("LSP OPEN Result reserved") !== 0)
    throw new YasProtocolError("LSP OPEN Result reserved byte is nonzero");
  value.backendCount = cursor.u16("LSP backend count");
  value.capabilities = cursor.u64("LSP capabilities");
  value.canonicalRoot = new Uint8Array(cursor.bytesU32("LSP canonical root"));
  value.extensions = decodeExtensions(
    cursor,
    new Set([g.YAS_LSP_OPEN_NO_BACKEND_DETAIL_EXTENSION]),
    "LSP OPEN Result extensions",
  );
  cursor.end("LSP OPEN Result");
  encodeLspOpenResult(value);
  return value;
}

export function encodeLspNoBackendDetail(detail: string): YasExtension {
  requireName(detail, g.YAS_LSP_MAX_DETAIL_BYTES, "LSP no-backend detail");
  return {
    tag: g.YAS_LSP_OPEN_NO_BACKEND_DETAIL_EXTENSION,
    required: true,
    value: new YasWriter().utf8U32(detail).finish(),
  };
}

export function decodeLspNoBackendDetail(
  extensions: readonly YasExtension[],
): string | undefined {
  encodeExtensions(extensions);
  if (
    extensions.some(
      (extension) =>
        extension.required &&
        extension.tag !== g.YAS_LSP_OPEN_NO_BACKEND_DETAIL_EXTENSION,
    )
  )
    throw new YasProtocolError("unknown required LSP OPEN Result extension");
  const extension = extensions.find(
    (entry) => entry.tag === g.YAS_LSP_OPEN_NO_BACKEND_DETAIL_EXTENSION,
  );
  if (!extension) return undefined;
  if (!extension.required)
    throw new YasProtocolError("optional LSP no-backend detail");
  const cursor = new YasCursor(extension.value);
  const detail = cursor.utf8U32("LSP no-backend detail");
  cursor.end("LSP no-backend detail");
  requireName(detail, g.YAS_LSP_MAX_DETAIL_BYTES, "LSP no-backend detail");
  return detail;
}

export function encodeLspClosed(value: YasLspClosed): Uint8Array {
  requireHandle(value.workspaceHandle, "LSP workspace handle");
  if (
    value.reason > g.YAS_LSP_CLOSED_RESOURCE_LIMIT ||
    utf8Length(value.detail) > g.YAS_LSP_MAX_DETAIL_BYTES
  )
    throw new YasProtocolError("invalid LSP CLOSED metadata");
  return new YasWriter()
    .u64(value.workspaceHandle)
    .u8(value.reason)
    .bytes(new Uint8Array(3))
    .utf8U32(value.detail)
    .finish();
}

export function decodeLspClosed(bytes: Uint8Array): YasLspClosed {
  const cursor = new YasCursor(bytes);
  const value = {
    workspaceHandle: cursor.u64("LSP workspace handle"),
    reason: cursor.u8("LSP close reason"),
    detail: "",
  };
  requireZero(cursor.take(3, "LSP CLOSED reserved"), "LSP CLOSED");
  value.detail = cursor.utf8U32("LSP CLOSED detail");
  cursor.end("LSP CLOSED");
  encodeLspClosed(value);
  return value;
}

export function encodeLspClose(
  workspaceHandle: bigint,
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  requireHandle(workspaceHandle, "LSP workspace handle");
  rejectRequiredExtensions(extensions, "LSP CLOSE");
  return new YasWriter()
    .u64(workspaceHandle)
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function decodeLspClose(bytes: Uint8Array): {
  workspaceHandle: bigint;
  extensions: YasExtension[];
} {
  const cursor = new YasCursor(bytes);
  const value = {
    workspaceHandle: cursor.u64("LSP workspace handle"),
    extensions: decodeExtensions(cursor, new Set(), "LSP CLOSE extensions"),
  };
  cursor.end("LSP CLOSE");
  encodeLspClose(value.workspaceHandle, value.extensions);
  return value;
}

export function encodeLspWatch(
  workspaceHandle: bigint,
  datasets: number,
  encodedStateWatch: Uint8Array,
): Uint8Array {
  requireHandle(workspaceHandle, "LSP workspace handle");
  if (datasets === 0 || datasets & ~g.YAS_LSP_WATCH_DATASETS)
    throw new YasProtocolError("invalid LSP WATCH datasets");
  return new YasWriter()
    .u64(workspaceHandle)
    .u16(datasets)
    .u16(0)
    .bytesU32(encodedStateWatch)
    .finish();
}

export function decodeLspWatch(bytes: Uint8Array): {
  workspaceHandle: bigint;
  datasets: number;
  encodedStateWatch: Uint8Array;
} {
  const cursor = new YasCursor(bytes);
  const workspaceHandle = cursor.u64("LSP workspace handle");
  const datasets = cursor.u16("LSP WATCH datasets");
  if (cursor.u16("LSP WATCH reserved") !== 0)
    throw new YasProtocolError("LSP WATCH reserved field is nonzero");
  const encodedStateWatch = new Uint8Array(cursor.bytesU32("LSP State WATCH"));
  cursor.end("LSP WATCH");
  encodeLspWatch(workspaceHandle, datasets, encodedStateWatch);
  return { workspaceHandle, datasets, encodedStateWatch };
}

export function encodeLspUnwatch(subscriptionId: number): Uint8Array {
  if (subscriptionId === 0)
    throw new YasProtocolError("zero LSP subscription ID");
  return new YasWriter().u32(subscriptionId).finish();
}

export function decodeLspUnwatch(bytes: Uint8Array): number {
  const cursor = new YasCursor(bytes);
  const value = cursor.u32("LSP subscription ID");
  cursor.end("LSP UNWATCH");
  encodeLspUnwatch(value);
  return value;
}

export function encodeLspQueryBody(value: YasLspQueryBody): Uint8Array {
  const body = new YasWriter();
  let kind: number;
  if (value.kind === "definition") {
    kind = g.YAS_LSP_QUERY_DEFINITION;
    body
      .bytes(encodeLspDocumentTarget(value.target))
      .bytes(encodeLspPosition(value.position));
  } else if (value.kind === "references") {
    if (value.flags & ~g.YAS_LSP_REFERENCES_FLAGS)
      throw new YasProtocolError("invalid LSP REFERENCES flags");
    kind = g.YAS_LSP_QUERY_REFERENCES;
    body
      .bytes(encodeLspDocumentTarget(value.target))
      .bytes(encodeLspPosition(value.position))
      .u16(value.flags)
      .u16(0);
  } else if (value.kind === "hover") {
    kind = g.YAS_LSP_QUERY_HOVER;
    body
      .bytes(encodeLspDocumentTarget(value.target))
      .bytes(encodeLspPosition(value.position));
  } else if (value.kind === "document-symbols") {
    kind = g.YAS_LSP_QUERY_DOCUMENT_SYMBOLS;
    body.bytes(encodeLspDocumentTarget(value.target));
  } else if (value.kind === "workspace-symbols") {
    kind = g.YAS_LSP_QUERY_WORKSPACE_SYMBOLS;
    requireName(
      value.query,
      g.YAS_LSP_MAX_QUERY_TEXT_BYTES,
      "LSP workspace symbol query",
    );
    body.utf8U16(value.query);
  } else if (value.kind === "completion") {
    kind = g.YAS_LSP_QUERY_COMPLETION;
    if (
      value.triggerKind > g.YAS_LSP_COMPLETION_TRIGGER_CHARACTER ||
      utf8Length(value.trigger) > g.YAS_LSP_MAX_TRIGGER_BYTES ||
      (value.triggerKind === g.YAS_LSP_COMPLETION_TRIGGER_CHARACTER) !==
        (value.trigger.length !== 0)
    )
      throw new YasProtocolError("invalid LSP completion trigger");
    body
      .bytes(encodeLspDocumentTarget(value.target))
      .bytes(encodeLspPosition(value.position))
      .u8(value.triggerKind)
      .bytes(new Uint8Array(3))
      .utf8U16(value.trigger);
  } else if (value.kind === "code-actions") {
    kind = g.YAS_LSP_QUERY_CODE_ACTIONS;
    if (value.diagnosticIds.length > g.YAS_LSP_MAX_DIAGNOSTIC_IDS)
      throw new YasProtocolError("too many LSP diagnostic IDs");
    body
      .bytes(encodeLspDocumentTarget(value.target))
      .bytes(encodeLspTextRange(value.range))
      .u16(value.diagnosticIds.length)
      .u16(0);
    for (const id of value.diagnosticIds) {
      requireHandle(id, "LSP diagnostic ID");
      body.u64(id);
    }
  } else if (value.kind === "formatting") {
    kind = g.YAS_LSP_QUERY_FORMATTING;
    if (value.tabWidth === 0 || value.flags & ~g.YAS_LSP_FORMATTING_FLAGS)
      throw new YasProtocolError("invalid LSP formatting options");
    body
      .bytes(encodeLspDocumentTarget(value.target))
      .u8(value.range ? 1 : 0)
      .bytes(new Uint8Array(3));
    if (value.range) body.bytes(encodeLspTextRange(value.range));
    body.u16(value.tabWidth).u16(value.flags);
  } else if (value.kind === "rename") {
    kind = g.YAS_LSP_QUERY_RENAME;
    requireName(
      value.newName,
      g.YAS_LSP_MAX_SYMBOL_NAME_BYTES,
      "LSP rename name",
    );
    body
      .bytes(encodeLspDocumentTarget(value.target))
      .bytes(encodeLspPosition(value.position))
      .utf8U16(value.newName);
  } else {
    kind = g.YAS_LSP_QUERY_SIGNATURE_HELP;
    body
      .bytes(encodeLspDocumentTarget(value.target))
      .bytes(encodeLspPosition(value.position));
  }
  return new YasWriter().u16(kind).u16(0).bytes(body.finish()).finish();
}

export function decodeLspQueryBody(bytes: Uint8Array): YasLspQueryBody {
  const cursor = new YasCursor(bytes);
  const kind = cursor.u16("LSP query kind");
  if (cursor.u16("LSP query flags") !== 0)
    throw new YasProtocolError("LSP query flags are nonzero");
  let value: YasLspQueryBody;
  if (kind === g.YAS_LSP_QUERY_DEFINITION)
    value = {
      kind: "definition",
      target: decodeDocumentTarget(cursor),
      position: decodePosition(cursor),
    };
  else if (kind === g.YAS_LSP_QUERY_REFERENCES) {
    const target = decodeDocumentTarget(cursor);
    const position = decodePosition(cursor);
    const flags = cursor.u16("LSP REFERENCES flags");
    if (cursor.u16("LSP REFERENCES reserved") !== 0)
      throw new YasProtocolError("LSP REFERENCES reserved field is nonzero");
    value = { kind: "references", target, position, flags };
  } else if (kind === g.YAS_LSP_QUERY_HOVER)
    value = {
      kind: "hover",
      target: decodeDocumentTarget(cursor),
      position: decodePosition(cursor),
    };
  else if (kind === g.YAS_LSP_QUERY_DOCUMENT_SYMBOLS)
    value = { kind: "document-symbols", target: decodeDocumentTarget(cursor) };
  else if (kind === g.YAS_LSP_QUERY_WORKSPACE_SYMBOLS)
    value = {
      kind: "workspace-symbols",
      query: cursor.utf8U16("LSP workspace symbol query"),
    };
  else if (kind === g.YAS_LSP_QUERY_COMPLETION) {
    const target = decodeDocumentTarget(cursor);
    const position = decodePosition(cursor);
    const triggerKind = cursor.u8("LSP completion trigger kind");
    requireZero(cursor.take(3, "LSP completion reserved"), "LSP completion");
    value = {
      kind: "completion",
      target,
      position,
      triggerKind,
      trigger: cursor.utf8U16("LSP completion trigger"),
    };
  } else if (kind === g.YAS_LSP_QUERY_CODE_ACTIONS) {
    const target = decodeDocumentTarget(cursor);
    const range = decodeRange(cursor);
    const count = cursor.u16("LSP diagnostic ID count");
    if (
      cursor.u16("LSP diagnostic IDs reserved") !== 0 ||
      count > g.YAS_LSP_MAX_DIAGNOSTIC_IDS ||
      count > Math.floor(cursor.remaining / 8)
    )
      throw new YasProtocolError("invalid LSP diagnostic ID count");
    const diagnosticIds: bigint[] = [];
    for (let index = 0; index < count; index++)
      diagnosticIds.push(cursor.u64("LSP diagnostic ID"));
    value = { kind: "code-actions", target, range, diagnosticIds };
  } else if (kind === g.YAS_LSP_QUERY_FORMATTING) {
    const target = decodeDocumentTarget(cursor);
    const present = cursor.u8("LSP formatting range presence");
    requireZero(cursor.take(3, "LSP formatting reserved"), "LSP formatting");
    if (present > 1)
      throw new YasProtocolError("invalid LSP formatting range presence");
    value = {
      kind: "formatting",
      target,
      range: present ? decodeRange(cursor) : undefined,
      tabWidth: cursor.u16("LSP formatting tab width"),
      flags: cursor.u16("LSP formatting flags"),
    };
  } else if (kind === g.YAS_LSP_QUERY_RENAME)
    value = {
      kind: "rename",
      target: decodeDocumentTarget(cursor),
      position: decodePosition(cursor),
      newName: cursor.utf8U16("LSP rename name"),
    };
  else if (kind === g.YAS_LSP_QUERY_SIGNATURE_HELP)
    value = {
      kind: "signature-help",
      target: decodeDocumentTarget(cursor),
      position: decodePosition(cursor),
    };
  else throw new YasProtocolError("unknown LSP query kind");
  cursor.end("LSP query body");
  encodeLspQueryBody(value);
  return value;
}

export function encodeLspQuery(value: YasLspQuery): Uint8Array {
  requireHandle(value.workspaceHandle, "LSP workspace handle");
  if (
    value.maxRecords > g.YAS_LSP_MAX_QUERY_RECORDS ||
    value.cursor.length > g.YAS_LSP_MAX_CURSOR_BYTES
  )
    throw new YasProtocolError("invalid LSP query page limits");
  rejectRequiredExtensions(value.extensions, "LSP QUERY");
  return new YasWriter()
    .u64(value.workspaceHandle)
    .u16(value.maxRecords)
    .u16(0)
    .bytesU16(value.cursor)
    .u64(value.initialReceiveCredit)
    .bytesU32(encodeLspQueryBody(value.body))
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeLspQuery(bytes: Uint8Array): YasLspQuery {
  const cursor = new YasCursor(bytes);
  const workspaceHandle = cursor.u64("LSP workspace handle");
  const maxRecords = cursor.u16("LSP maximum records");
  if (cursor.u16("LSP QUERY reserved") !== 0)
    throw new YasProtocolError("LSP QUERY reserved field is nonzero");
  const value = {
    workspaceHandle,
    maxRecords,
    cursor: new Uint8Array(cursor.bytesU16("LSP query cursor")),
    initialReceiveCredit: cursor.u64("LSP initial receive credit"),
    body: decodeLspQueryBody(cursor.bytesU32("LSP query body")),
    extensions: decodeExtensions(cursor, new Set(), "LSP QUERY extensions"),
  };
  cursor.end("LSP QUERY");
  encodeLspQuery(value);
  return value;
}

export function encodeLspBufferIdentity(
  value: YasLspBufferIdentity,
): Uint8Array {
  validateBufferIdentity(value);
  return new YasWriter()
    .u64(value.bufferHandle)
    .u64(value.bufferRevision)
    .u64(value.workspaceRevision)
    .u64(value.byteLength)
    .bytes(value.contentHash)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeLspBufferIdentity(
  bytes: Uint8Array,
): YasLspBufferIdentity {
  const cursor = new YasCursor(bytes);
  const value = {
    bufferHandle: cursor.u64("LSP buffer handle"),
    bufferRevision: cursor.u64("LSP buffer revision"),
    workspaceRevision: cursor.u64("LSP workspace revision"),
    byteLength: cursor.u64("LSP buffer byte length"),
    contentHash: new Uint8Array(cursor.take(32, "LSP buffer content hash")),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "LSP buffer identity extensions",
    ),
  };
  cursor.end("LSP buffer identity");
  validateBufferIdentity(value);
  return value;
}

export function encodeLspBufferPut(value: YasLspBufferPut): Uint8Array {
  requireHandle(value.workspaceHandle, "LSP workspace handle");
  requireOperationId(value.operationId);
  requireDocumentPath(value.path);
  if (value.content.length > g.YAS_LSP_MAX_INLINE_BUFFER_BYTES)
    throw new YasProtocolError("LSP inline buffer exceeds limit");
  rejectRequiredExtensions(value.extensions, "LSP BUFFER_PUT");
  return new YasWriter()
    .u64(value.workspaceHandle)
    .bytes(value.operationId)
    .u64(value.expectedRevision)
    .bytesU32(encodeFsPath(value.path))
    .bytesU32(value.content)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeLspBufferPut(bytes: Uint8Array): YasLspBufferPut {
  const cursor = new YasCursor(bytes);
  const value = {
    workspaceHandle: cursor.u64("LSP workspace handle"),
    operationId: new Uint8Array(cursor.take(16, "LSP operation ID")),
    expectedRevision: cursor.u64("LSP expected buffer revision"),
    path: decodeFsPath(cursor.bytesU32("LSP buffer path")),
    content: new Uint8Array(cursor.bytesU32("LSP buffer content")),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "LSP BUFFER_PUT extensions",
    ),
  };
  cursor.end("LSP BUFFER_PUT");
  encodeLspBufferPut(value);
  return value;
}

export function encodeLspBufferBegin(value: YasLspBufferBegin): Uint8Array {
  requireHandle(value.workspaceHandle, "LSP workspace handle");
  requireDocumentPath(value.path);
  requireHash(value.contentHash);
  if (
    value.byteLength === 0n ||
    value.byteLength > BigInt(g.YAS_LSP_MAX_BUFFER_BYTES) ||
    value.initialSendCredit === 0n
  )
    throw new YasProtocolError("invalid LSP staged buffer metadata");
  rejectRequiredExtensions(value.extensions, "LSP BUFFER_BEGIN");
  return new YasWriter()
    .u64(value.workspaceHandle)
    .u64(value.expectedRevision)
    .bytesU32(encodeFsPath(value.path))
    .u64(value.byteLength)
    .bytes(value.contentHash)
    .u64(value.initialSendCredit)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeLspBufferBegin(bytes: Uint8Array): YasLspBufferBegin {
  const cursor = new YasCursor(bytes);
  const value = {
    workspaceHandle: cursor.u64("LSP workspace handle"),
    expectedRevision: cursor.u64("LSP expected buffer revision"),
    path: decodeFsPath(cursor.bytesU32("LSP buffer path")),
    byteLength: cursor.u64("LSP buffer byte length"),
    contentHash: new Uint8Array(cursor.take(32, "LSP content hash")),
    initialSendCredit: cursor.u64("LSP initial send credit"),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "LSP BUFFER_BEGIN extensions",
    ),
  };
  cursor.end("LSP BUFFER_BEGIN");
  encodeLspBufferBegin(value);
  return value;
}

export function encodeLspBufferBeginResult(
  value: YasLspBufferBeginResult,
): Uint8Array {
  requireHandle(value.stagingHandle, "LSP staging handle");
  validateBufferDescriptor(value.descriptor);
  requireTransferUploadStage(
    value.descriptor,
    value.stagingHandle,
    "LSP buffer descriptor",
  );
  rejectRequiredExtensions(value.extensions, "LSP BUFFER_BEGIN Result");
  return new YasWriter()
    .u64(value.stagingHandle)
    .bytesU32(encodeTransferDescriptor(value.descriptor))
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeLspBufferBeginResult(
  bytes: Uint8Array,
): YasLspBufferBeginResult {
  const cursor = new YasCursor(bytes);
  const value = {
    stagingHandle: cursor.u64("LSP staging handle"),
    descriptor: decodeTransferDescriptor(
      new YasCursor(cursor.bytesU32("LSP buffer Transfer descriptor")),
    ),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "LSP BUFFER_BEGIN Result extensions",
    ),
  };
  cursor.end("LSP BUFFER_BEGIN Result");
  encodeLspBufferBeginResult(value);
  return value;
}

export function encodeLspBufferCommit(value: YasLspBufferCommit): Uint8Array {
  requireHandle(value.stagingHandle, "LSP staging handle");
  requireOperationId(value.operationId);
  rejectRequiredExtensions(value.extensions, "LSP BUFFER_COMMIT");
  return new YasWriter()
    .u64(value.stagingHandle)
    .bytes(value.operationId)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeLspBufferCommit(bytes: Uint8Array): YasLspBufferCommit {
  const cursor = new YasCursor(bytes);
  const value = {
    stagingHandle: cursor.u64("LSP staging handle"),
    operationId: new Uint8Array(cursor.take(16, "LSP operation ID")),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "LSP BUFFER_COMMIT extensions",
    ),
  };
  cursor.end("LSP BUFFER_COMMIT");
  encodeLspBufferCommit(value);
  return value;
}

export function encodeLspBufferClose(value: YasLspBufferClose): Uint8Array {
  requireHandle(value.bufferHandle, "LSP buffer handle");
  requireRevision(value.expectedRevision, "LSP expected buffer revision");
  requireOperationId(value.operationId);
  rejectRequiredExtensions(value.extensions, "LSP BUFFER_CLOSE");
  return new YasWriter()
    .u64(value.bufferHandle)
    .u64(value.expectedRevision)
    .bytes(value.operationId)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeLspBufferClose(bytes: Uint8Array): YasLspBufferClose {
  const cursor = new YasCursor(bytes);
  const value = {
    bufferHandle: cursor.u64("LSP buffer handle"),
    expectedRevision: cursor.u64("LSP expected buffer revision"),
    operationId: new Uint8Array(cursor.take(16, "LSP operation ID")),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "LSP BUFFER_CLOSE extensions",
    ),
  };
  cursor.end("LSP BUFFER_CLOSE");
  encodeLspBufferClose(value);
  return value;
}

export function encodeLspListServers(
  workspaceHandle = 0n,
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  rejectRequiredExtensions(extensions, "LSP LIST_SERVERS");
  return new YasWriter()
    .u64(workspaceHandle)
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function decodeLspListServers(bytes: Uint8Array): {
  workspaceHandle: bigint;
  extensions: YasExtension[];
} {
  const cursor = new YasCursor(bytes);
  const value = {
    workspaceHandle: cursor.u64("LSP workspace handle"),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "LSP LIST_SERVERS extensions",
    ),
  };
  cursor.end("LSP LIST_SERVERS");
  encodeLspListServers(value.workspaceHandle, value.extensions);
  return value;
}

export function encodeLspStopServer(value: YasLspStopServer): Uint8Array {
  requireHandle(value.serverHandle, "LSP server handle");
  requireRevision(value.generation, "LSP server generation");
  requireOperationId(value.operationId);
  rejectRequiredExtensions(value.extensions, "LSP STOP_SERVER");
  return new YasWriter()
    .u64(value.serverHandle)
    .u64(value.generation)
    .bytes(value.operationId)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeLspStopServer(bytes: Uint8Array): YasLspStopServer {
  const cursor = new YasCursor(bytes);
  const value = {
    serverHandle: cursor.u64("LSP server handle"),
    generation: cursor.u64("LSP server generation"),
    operationId: new Uint8Array(cursor.take(16, "LSP operation ID")),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "LSP STOP_SERVER extensions",
    ),
  };
  cursor.end("LSP STOP_SERVER");
  encodeLspStopServer(value);
  return value;
}

export function encodeLspLocationRecord(
  value: Omit<YasLspLocationRecord, "kind"> | YasLspLocationRecord,
): Uint8Array {
  requireDocumentPath(value.path);
  requireHash(value.contentHash);
  if (value.flags & ~g.YAS_LSP_LOCATION_FLAGS)
    throw new YasProtocolError("invalid LSP location flags");
  return new YasWriter()
    .bytesU32(encodeFsPath(value.path))
    .u64(value.documentRevision)
    .bytes(value.contentHash)
    .bytes(encodeLspTextRange(value.range))
    .u16(value.flags)
    .u16(0)
    .finish();
}

export function decodeLspLocationRecord(
  bytes: Uint8Array,
): YasLspLocationRecord {
  const cursor = new YasCursor(bytes);
  const value: YasLspLocationRecord = {
    kind: "location",
    path: decodeFsPath(cursor.bytesU32("LSP location path")),
    documentRevision: cursor.u64("LSP location document revision"),
    contentHash: new Uint8Array(cursor.take(32, "LSP location content hash")),
    range: decodeRange(cursor),
    flags: cursor.u16("LSP location flags"),
  };
  if (cursor.u16("LSP location reserved") !== 0)
    throw new YasProtocolError("LSP location reserved field is nonzero");
  cursor.end("LSP location");
  encodeLspLocationRecord(value);
  return value;
}

export function encodeLspHoverRecord(
  value: Omit<YasLspHoverRecord, "kind"> | YasLspHoverRecord,
): Uint8Array {
  if (
    value.markupKind > g.YAS_LSP_MARKUP_MARKDOWN ||
    value.content.length > g.YAS_LSP_MAX_MARKUP_BYTES
  )
    throw new YasProtocolError("invalid LSP hover markup");
  return new YasWriter()
    .bytesU32(encodeLspLocationRecord(value.target))
    .u8(value.markupKind)
    .bytes(new Uint8Array(3))
    .bytesU32(value.content)
    .finish();
}

export function decodeLspHoverRecord(bytes: Uint8Array): YasLspHoverRecord {
  const cursor = new YasCursor(bytes);
  const target = decodeLspLocationRecord(cursor.bytesU32("LSP hover target"));
  const markupKind = cursor.u8("LSP hover markup kind");
  requireZero(cursor.take(3, "LSP hover reserved"), "LSP hover");
  const value: YasLspHoverRecord = {
    kind: "hover",
    target,
    markupKind,
    content: new Uint8Array(cursor.bytesU32("LSP hover content")),
  };
  cursor.end("LSP hover");
  encodeLspHoverRecord(value);
  return value;
}

export function encodeLspSymbolRecord(
  value: Omit<YasLspSymbolRecord, "kind"> | YasLspSymbolRecord,
): Uint8Array {
  if (
    value.symbolKind > g.YAS_LSP_SYMBOL_TYPE_PARAMETER ||
    value.flags & ~g.YAS_LSP_SYMBOL_FLAGS ||
    Boolean(value.path) !== Boolean(value.contentHash)
  )
    throw new YasProtocolError("invalid LSP symbol metadata");
  requireName(value.name, g.YAS_LSP_MAX_SYMBOL_NAME_BYTES, "LSP symbol name");
  if (utf8Length(value.detail) > g.YAS_LSP_MAX_DETAIL_BYTES)
    throw new YasProtocolError("LSP symbol detail exceeds limit");
  return new YasWriter()
    .u16(value.symbolKind)
    .u16(value.flags)
    .u16(value.depth)
    .u16(0)
    .utf8U16(value.name)
    .utf8U16(value.detail)
    .bytesU32(value.path ? encodeFsPath(value.path) : new Uint8Array())
    .u8(value.contentHash ? 1 : 0)
    .bytes(new Uint8Array(3))
    .bytes(value.contentHash ?? new Uint8Array())
    .bytes(encodeLspTextRange(value.range))
    .bytes(encodeLspTextRange(value.selectionRange))
    .finish();
}

export function decodeLspSymbolRecord(bytes: Uint8Array): YasLspSymbolRecord {
  return decodeLspSymbolRecordImpl(bytes);
}

function decodeLspSymbolRecordImpl(bytes: Uint8Array): YasLspSymbolRecord {
  const cursor = new YasCursor(bytes);
  const symbolKind = cursor.u16("LSP symbol kind");
  const flags = cursor.u16("LSP symbol flags");
  const depth = cursor.u16("LSP symbol depth");
  if (cursor.u16("LSP symbol reserved") !== 0)
    throw new YasProtocolError("LSP symbol reserved field is nonzero");
  const name = cursor.utf8U16("LSP symbol name");
  const detail = cursor.utf8U16("LSP symbol detail");
  const encodedPath = cursor.bytesU32("LSP symbol path");
  const hashPresent = cursor.u8("LSP symbol content hash presence");
  requireZero(cursor.take(3, "LSP symbol reserved"), "LSP symbol");
  if (hashPresent > 1)
    throw new YasProtocolError("invalid LSP symbol content hash presence");
  const value: YasLspSymbolRecord = {
    kind: "symbol",
    symbolKind,
    flags,
    depth,
    name,
    detail,
    path: encodedPath.length ? decodeFsPath(encodedPath) : undefined,
    contentHash: hashPresent
      ? new Uint8Array(cursor.take(32, "LSP symbol content hash"))
      : undefined,
    range: decodeRange(cursor),
    selectionRange: decodeRange(cursor),
  };
  cursor.end("LSP symbol");
  encodeLspSymbolRecord(value);
  return value;
}

export function encodeLspCompletionRecord(
  value: Omit<YasLspCompletionRecord, "kind"> | YasLspCompletionRecord,
): Uint8Array {
  if (
    value.itemKind > g.YAS_LSP_COMPLETION_TYPE_PARAMETER ||
    value.flags & ~g.YAS_LSP_COMPLETION_FLAGS ||
    value.insertText.length > g.YAS_LSP_MAX_EDIT_BYTES
  )
    throw new YasProtocolError("invalid LSP completion item");
  requireName(
    value.label,
    g.YAS_LSP_MAX_SYMBOL_NAME_BYTES,
    "LSP completion label",
  );
  if (
    utf8Length(value.detail) > g.YAS_LSP_MAX_DETAIL_BYTES ||
    utf8Length(value.filterText) > g.YAS_LSP_MAX_SYMBOL_NAME_BYTES
  )
    throw new YasProtocolError("LSP completion text exceeds limit");
  const writer = new YasWriter()
    .u16(value.itemKind)
    .u16(value.flags)
    .utf8U16(value.label)
    .utf8U16(value.detail)
    .utf8U16(value.filterText)
    .bytesU32(value.insertText)
    .u8(value.replacementRange ? 1 : 0)
    .bytes(new Uint8Array(3));
  if (value.replacementRange)
    writer.bytes(encodeLspTextRange(value.replacementRange));
  return writer.finish();
}

export function decodeLspCompletionRecord(
  bytes: Uint8Array,
): YasLspCompletionRecord {
  const cursor = new YasCursor(bytes);
  const itemKind = cursor.u16("LSP completion item kind");
  const flags = cursor.u16("LSP completion flags");
  const label = cursor.utf8U16("LSP completion label");
  const detail = cursor.utf8U16("LSP completion detail");
  const filterText = cursor.utf8U16("LSP completion filter text");
  const insertText = new Uint8Array(cursor.bytesU32("LSP completion text"));
  const present = cursor.u8("LSP completion range presence");
  requireZero(cursor.take(3, "LSP completion reserved"), "LSP completion");
  if (present > 1)
    throw new YasProtocolError("invalid LSP completion range presence");
  const value: YasLspCompletionRecord = {
    kind: "completion",
    itemKind,
    flags,
    label,
    detail,
    filterText,
    insertText,
    replacementRange: present ? decodeRange(cursor) : undefined,
  };
  cursor.end("LSP completion");
  encodeLspCompletionRecord(value);
  return value;
}

export function encodeLspEditRecord(
  value: Omit<YasLspEditRecord, "kind"> | YasLspEditRecord,
): Uint8Array {
  requireDocumentPath(value.path);
  requireHash(value.expectedContentHash);
  if (value.replacement.length > g.YAS_LSP_MAX_EDIT_BYTES)
    throw new YasProtocolError("LSP replacement exceeds limit");
  return new YasWriter()
    .bytesU32(encodeFsPath(value.path))
    .u64(value.expectedRevision)
    .bytes(value.expectedContentHash)
    .bytes(encodeLspTextRange(value.range))
    .bytesU32(value.replacement)
    .finish();
}

export function decodeLspEditRecord(bytes: Uint8Array): YasLspEditRecord {
  const cursor = new YasCursor(bytes);
  const value = decodeEdit(cursor);
  cursor.end("LSP edit");
  encodeLspEditRecord(value);
  return value;
}

export function encodeLspSignatureRecord(
  value: Omit<YasLspSignatureRecord, "kind"> | YasLspSignatureRecord,
): Uint8Array {
  const noParameter =
    value.activeParameter === g.YAS_LSP_SIGNATURE_NO_ACTIVE_PARAMETER;
  const labelBytes = encoder.encode(value.label);
  if (
    value.flags & ~g.YAS_LSP_SIGNATURE_FLAGS ||
    labelBytes.length === 0 ||
    labelBytes.length > g.YAS_LSP_MAX_SYMBOL_NAME_BYTES ||
    utf8Length(value.documentation) > g.YAS_LSP_MAX_MARKUP_BYTES ||
    (noParameter && (value.parameterStart !== 0 || value.parameterEnd !== 0)) ||
    (!noParameter &&
      (value.parameterStart > value.parameterEnd ||
        value.parameterEnd > labelBytes.length))
  )
    throw new YasProtocolError("invalid LSP signature");
  return new YasWriter()
    .u16(value.flags)
    .u16(value.activeParameter)
    .u32(value.parameterStart)
    .u32(value.parameterEnd)
    .utf8U16(value.label)
    .utf8U32(value.documentation)
    .finish();
}

export function decodeLspSignatureRecord(
  bytes: Uint8Array,
): YasLspSignatureRecord {
  const cursor = new YasCursor(bytes);
  const value: YasLspSignatureRecord = {
    kind: "signature",
    flags: cursor.u16("LSP signature flags"),
    activeParameter: cursor.u16("LSP active parameter"),
    parameterStart: cursor.u32("LSP parameter start"),
    parameterEnd: cursor.u32("LSP parameter end"),
    label: cursor.utf8U16("LSP signature label"),
    documentation: cursor.utf8U32("LSP signature documentation"),
  };
  cursor.end("LSP signature");
  encodeLspSignatureRecord(value);
  return value;
}

export function encodeLspActionRecord(
  value: Omit<YasLspActionRecord, "kind"> | YasLspActionRecord,
): Uint8Array {
  requireName(value.title, g.YAS_LSP_MAX_DETAIL_BYTES, "LSP action title");
  if (
    utf8Length(value.actionKind) > g.YAS_LSP_MAX_ACTION_KIND_BYTES ||
    value.flags & ~g.YAS_LSP_ACTION_FLAGS ||
    value.edits.length > g.YAS_LSP_MAX_EDITS_PER_ACTION ||
    utf8Length(value.disabledReason) > g.YAS_LSP_MAX_DETAIL_BYTES ||
    Boolean(value.flags & g.YAS_LSP_ACTION_DISABLED) !==
      Boolean(value.disabledReason)
  )
    throw new YasProtocolError("invalid LSP code action");
  const writer = new YasWriter()
    .utf8U16(value.title)
    .utf8U16(value.actionKind)
    .u16(value.flags)
    .u16(value.edits.length);
  for (const edit of value.edits) writer.bytesU32(encodeLspEditRecord(edit));
  return writer.utf8U32(value.disabledReason).finish();
}

export function decodeLspActionRecord(bytes: Uint8Array): YasLspActionRecord {
  const cursor = new YasCursor(bytes);
  const title = cursor.utf8U16("LSP action title");
  const actionKind = cursor.utf8U16("LSP action kind");
  const flags = cursor.u16("LSP action flags");
  const count = cursor.u16("LSP action edit count");
  if (
    count > g.YAS_LSP_MAX_EDITS_PER_ACTION ||
    count > Math.floor(cursor.remaining / 4)
  )
    throw new YasProtocolError("invalid LSP action edit count");
  const edits: YasLspEditRecord[] = [];
  for (let index = 0; index < count; index++)
    edits.push(decodeLspEditRecord(cursor.bytesU32("LSP action edit")));
  const value: YasLspActionRecord = {
    kind: "action",
    title,
    actionKind,
    flags,
    edits,
    disabledReason: cursor.utf8U32("LSP action disabled reason"),
  };
  cursor.end("LSP action");
  encodeLspActionRecord(value);
  return value;
}

export function encodeLspQueryRecord(value: YasLspQueryRecord): Uint8Array {
  let kind: number;
  let body: Uint8Array;
  if (value.kind === "location") {
    kind = g.YAS_LSP_RESULT_LOCATION;
    body = encodeLspLocationRecord(value);
  } else if (value.kind === "hover") {
    kind = g.YAS_LSP_RESULT_HOVER;
    body = encodeLspHoverRecord(value);
  } else if (value.kind === "symbol") {
    kind = g.YAS_LSP_RESULT_SYMBOL;
    body = encodeLspSymbolRecord(value);
  } else if (value.kind === "completion") {
    kind = g.YAS_LSP_RESULT_COMPLETION;
    body = encodeLspCompletionRecord(value);
  } else if (value.kind === "action") {
    kind = g.YAS_LSP_RESULT_ACTION;
    body = encodeLspActionRecord(value);
  } else if (value.kind === "edit") {
    kind = g.YAS_LSP_RESULT_EDIT;
    body = encodeLspEditRecord(value);
  } else {
    kind = g.YAS_LSP_RESULT_SIGNATURE;
    body = encodeLspSignatureRecord(value);
  }
  return encodeTypedRecord({ kind, flags: 0, body });
}

export function decodeLspQueryRecord(
  cursor: YasCursor,
): YasLspQueryRecord | undefined {
  const length = cursor.u32("LSP query record length");
  const record = cursor.sub(length, "LSP query record");
  const kind = record.u16("LSP query record kind");
  const flags = record.u16("LSP query record flags");
  if (flags & ~1) throw new YasProtocolError("invalid LSP query record flags");
  const body = new Uint8Array(record.take(record.remaining));
  if (kind === g.YAS_LSP_RESULT_LOCATION) return decodeLspLocationRecord(body);
  if (kind === g.YAS_LSP_RESULT_HOVER) return decodeLspHoverRecord(body);
  if (kind === g.YAS_LSP_RESULT_SYMBOL) return decodeLspSymbolRecordImpl(body);
  if (kind === g.YAS_LSP_RESULT_COMPLETION)
    return decodeLspCompletionRecord(body);
  if (kind === g.YAS_LSP_RESULT_ACTION) return decodeLspActionRecord(body);
  if (kind === g.YAS_LSP_RESULT_EDIT) return decodeLspEditRecord(body);
  if (kind === g.YAS_LSP_RESULT_SIGNATURE)
    return decodeLspSignatureRecord(body);
  if (flags & 1)
    throw new YasProtocolError("unknown required LSP query record");
  return undefined;
}

export function encodeLspQueryPage(value: YasLspQueryPageWire): Uint8Array {
  const ok = value.queryStatus === g.YAS_STATUS_OK;
  if (
    value.queryStatus > g.YAS_STATUS_INTERNAL ||
    value.flags & ~g.YAS_LSP_PAGE_FLAGS ||
    utf8Length(value.detail) > g.YAS_LSP_MAX_DETAIL_BYTES ||
    (ok && value.detail.length !== 0) ||
    (!ok &&
      (!(value.flags & g.YAS_LSP_PAGE_INCOMPLETE) ||
        value.detail.length === 0)) ||
    Boolean(value.flags & g.YAS_LSP_PAGE_TRUNCATED) !==
      (value.nextCursor.length !== 0) ||
    value.nextCursor.length > g.YAS_LSP_MAX_CURSOR_BYTES
  )
    throw new YasProtocolError("LSP query cursor exceeds limit");
  rejectRequiredExtensions(value.extensions, "LSP query page");
  const writer = new YasWriter()
    .u16(value.queryStatus)
    .u16(value.flags)
    .utf8U32(value.detail)
    .bytesU16(value.nextCursor)
    .u64(value.totalHint)
    .u8(
      value.delivery.kind === "inline"
        ? g.YAS_LSP_PAGE_INLINE
        : g.YAS_LSP_PAGE_TRANSFER,
    )
    .bytes(new Uint8Array(3));
  if (value.delivery.kind === "inline") {
    if (value.delivery.records.length > g.YAS_LSP_MAX_QUERY_RECORDS)
      throw new YasProtocolError("too many LSP query records");
    const recordWriter = new YasWriter();
    for (const record of value.delivery.records)
      recordWriter.bytes(encodeLspQueryRecord(record));
    const stream = recordWriter.finish();
    if (stream.length > g.YAS_LSP_MAX_QUERY_BYTES)
      throw new YasProtocolError("LSP query page exceeds byte limit");
    writer.u16(value.delivery.records.length).u16(0).bytesU32(stream);
  } else {
    validateQueryDescriptor(value.delivery.descriptor);
    writer.bytesU32(encodeTransferDescriptor(value.delivery.descriptor));
  }
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeLspQueryPage(bytes: Uint8Array): YasLspQueryPageWire {
  const cursor = new YasCursor(bytes);
  const queryStatus = cursor.u16("LSP query status");
  const flags = cursor.u16("LSP query page flags");
  const detail = cursor.utf8U32("LSP query detail");
  const nextCursor = new Uint8Array(cursor.bytesU16("LSP query cursor"));
  const totalHint = cursor.u64("LSP query total hint");
  const deliveryKind = cursor.u8("LSP query delivery");
  requireZero(cursor.take(3, "LSP query page reserved"), "LSP query page");
  let delivery: YasLspPageDelivery;
  if (deliveryKind === g.YAS_LSP_PAGE_INLINE) {
    const count = cursor.u16("LSP query record count");
    if (
      cursor.u16("LSP query inline reserved") !== 0 ||
      count > g.YAS_LSP_MAX_QUERY_RECORDS
    )
      throw new YasProtocolError("invalid LSP query record count");
    const recordBytes = cursor.bytesU32("LSP query record stream");
    if (
      recordBytes.length > g.YAS_LSP_MAX_QUERY_BYTES ||
      count > Math.floor(recordBytes.length / 8)
    )
      throw new YasProtocolError("invalid LSP query record stream");
    const recordsCursor = new YasCursor(recordBytes);
    const records: YasLspQueryRecord[] = [];
    for (let index = 0; index < count; index++) {
      const record = decodeLspQueryRecord(recordsCursor);
      if (record) records.push(record);
    }
    recordsCursor.end("LSP query record stream");
    delivery = { kind: "inline", records };
  } else if (deliveryKind === g.YAS_LSP_PAGE_TRANSFER) {
    const descriptorCursor = new YasCursor(
      cursor.bytesU32("LSP query Transfer descriptor"),
    );
    const descriptor = decodeTransferDescriptor(descriptorCursor);
    descriptorCursor.end("LSP query Transfer descriptor");
    validateQueryDescriptor(descriptor);
    delivery = { kind: "transfer", descriptor };
  } else throw new YasProtocolError("unknown LSP query page delivery");
  const value = {
    queryStatus,
    flags,
    detail,
    nextCursor,
    totalHint,
    delivery,
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "LSP query page extensions",
    ),
  };
  cursor.end("LSP query page");
  encodeLspQueryPage(value);
  return value;
}

function encodeLspServerRecordInContext(
  value: YasLspServerRecord,
  allowDetachedWorkspace: boolean,
): Uint8Array {
  validateServer(value, allowDetachedWorkspace);
  return new YasWriter()
    .u64(value.serverHandle)
    .u64(value.generation)
    .u64(value.serverRevision)
    .u64(value.workspaceHandle)
    .u8(value.phase)
    .u8(value.progressPercent)
    .u16(0)
    .u32(value.epoch)
    .u32(value.refusedEdits)
    .u32(0)
    .u64(value.rssBytes)
    .u64(value.capabilities)
    .utf8U16(value.language)
    .utf8U16(value.profile)
    .utf8U16(value.backendId)
    .utf8U32(value.lastMessage)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function encodeLspServerRecord(value: YasLspServerRecord): Uint8Array {
  return encodeLspServerRecordInContext(value, false);
}

function decodeLspServerRecordInContext(
  bytes: Uint8Array,
  allowDetachedWorkspace: boolean,
): YasLspServerRecord {
  const cursor = new YasCursor(bytes);
  const serverHandle = cursor.u64("LSP server handle");
  const generation = cursor.u64("LSP server generation");
  const serverRevision = cursor.u64("LSP server revision");
  const workspaceHandle = cursor.u64("LSP workspace handle");
  const phase = cursor.u8("LSP server phase");
  const progressPercent = cursor.u8("LSP server progress");
  if (cursor.u16("LSP server reserved") !== 0)
    throw new YasProtocolError("LSP server reserved field is nonzero");
  const epoch = cursor.u32("LSP server epoch");
  const refusedEdits = cursor.u32("LSP server refused edits");
  if (cursor.u32("LSP server reserved") !== 0)
    throw new YasProtocolError("LSP server reserved field is nonzero");
  const rssBytes = cursor.u64("LSP server RSS bytes");
  const value = {
    serverHandle,
    generation,
    serverRevision,
    workspaceHandle,
    phase,
    progressPercent,
    epoch,
    refusedEdits,
    rssBytes,
    capabilities: cursor.u64("LSP server capabilities"),
    language: cursor.utf8U16("LSP server language"),
    profile: cursor.utf8U16("LSP server profile"),
    backendId: cursor.utf8U16("LSP backend ID"),
    lastMessage: cursor.utf8U32("LSP server last message"),
    extensions: decodeExtensions(cursor, new Set(), "LSP server extensions"),
  };
  cursor.end("LSP server");
  validateServer(value, allowDetachedWorkspace);
  return value;
}

export function decodeLspServerRecord(bytes: Uint8Array): YasLspServerRecord {
  return decodeLspServerRecordInContext(bytes, false);
}

export function encodeLspServerList(
  servers: readonly YasLspServerRecord[],
): Uint8Array {
  if (servers.length > g.YAS_LSP_MAX_SERVERS)
    throw new YasProtocolError("too many LSP servers");
  const writer = new YasWriter().u16(servers.length).u16(0);
  for (const server of servers)
    writer.bytesU32(encodeLspServerRecordInContext(server, true));
  return writer.finish();
}

export function decodeLspServerList(bytes: Uint8Array): YasLspServerRecord[] {
  const cursor = new YasCursor(bytes);
  const count = cursor.u16("LSP server count");
  if (
    cursor.u16("LSP server list reserved") !== 0 ||
    count > g.YAS_LSP_MAX_SERVERS ||
    count > Math.floor(cursor.remaining / 4)
  )
    throw new YasProtocolError("invalid LSP server count");
  const servers: YasLspServerRecord[] = [];
  for (let index = 0; index < count; index++)
    servers.push(
      decodeLspServerRecordInContext(cursor.bytesU32("LSP server"), true),
    );
  cursor.end("LSP server list");
  return servers;
}

export function encodeLspDiagnostic(value: YasLspDiagnostic): Uint8Array {
  validateDiagnostic(value);
  return new YasWriter()
    .u64(value.diagnosticId)
    .u8(value.severity)
    .u8(0)
    .u16(value.tags)
    .bytes(encodeLspTextRange(value.range))
    .utf8U16(value.code)
    .utf8U16(value.source)
    .utf8U32(value.message)
    .finish();
}

export function decodeLspDiagnostic(bytes: Uint8Array): YasLspDiagnostic {
  const cursor = new YasCursor(bytes);
  const diagnosticId = cursor.u64("LSP diagnostic ID");
  const severity = cursor.u8("LSP diagnostic severity");
  if (cursor.u8("LSP diagnostic reserved") !== 0)
    throw new YasProtocolError("LSP diagnostic reserved byte is nonzero");
  const value = {
    diagnosticId,
    severity,
    tags: cursor.u16("LSP diagnostic tags"),
    range: decodeRange(cursor),
    code: cursor.utf8U16("LSP diagnostic code"),
    source: cursor.utf8U16("LSP diagnostic source"),
    message: cursor.utf8U32("LSP diagnostic message"),
  };
  cursor.end("LSP diagnostic");
  validateDiagnostic(value);
  return value;
}

export function encodeLspDiagnosticRecord(
  value: YasLspDiagnosticRecord,
): Uint8Array {
  requireDocumentPath(value.path);
  requireRevision(value.diagnosticsRevision, "LSP diagnostics revision");
  requireHash(value.contentHash);
  if (value.diagnostics.length > g.YAS_LSP_MAX_DIAGNOSTICS_PER_FILE)
    throw new YasProtocolError("too many LSP diagnostics");
  rejectRequiredExtensions(value.extensions, "LSP diagnostics");
  const writer = new YasWriter()
    .bytesU32(encodeFsPath(value.path))
    .u64(value.documentRevision)
    .bytes(value.contentHash)
    .u64(value.diagnosticsRevision)
    .u16(value.diagnostics.length)
    .u16(0);
  for (const diagnostic of value.diagnostics)
    writer.bytesU32(encodeLspDiagnostic(diagnostic));
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeLspDiagnosticRecord(
  bytes: Uint8Array,
): YasLspDiagnosticRecord {
  const cursor = new YasCursor(bytes);
  const path = decodeFsPath(cursor.bytesU32("LSP diagnostic path"));
  const documentRevision = cursor.u64("LSP document revision");
  const contentHash = new Uint8Array(
    cursor.take(32, "LSP diagnostic content hash"),
  );
  const diagnosticsRevision = cursor.u64("LSP diagnostics revision");
  const count = cursor.u16("LSP diagnostic count");
  if (
    cursor.u16("LSP diagnostics reserved") !== 0 ||
    count > g.YAS_LSP_MAX_DIAGNOSTICS_PER_FILE ||
    count > Math.floor(cursor.remaining / 4)
  )
    throw new YasProtocolError("invalid LSP diagnostic count");
  const diagnostics: YasLspDiagnostic[] = [];
  for (let index = 0; index < count; index++)
    diagnostics.push(decodeLspDiagnostic(cursor.bytesU32("LSP diagnostic")));
  const value = {
    path,
    documentRevision,
    contentHash,
    diagnosticsRevision,
    diagnostics,
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "LSP diagnostics extensions",
    ),
  };
  cursor.end("LSP diagnostics");
  encodeLspDiagnosticRecord(value);
  return value;
}

export function encodeLspBufferRecord(value: YasLspBufferRecord): Uint8Array {
  requireHandle(value.workspaceHandle, "LSP workspace handle");
  requireHandle(value.bufferHandle, "LSP buffer handle");
  requireRevision(value.bufferRevision, "LSP buffer revision");
  requireDocumentPath(value.path);
  requireHash(value.contentHash);
  if (value.byteLength > BigInt(g.YAS_LSP_MAX_BUFFER_BYTES))
    throw new YasProtocolError("LSP buffer exceeds limit");
  rejectRequiredExtensions(value.extensions, "LSP buffer state");
  return new YasWriter()
    .u64(value.workspaceHandle)
    .u64(value.bufferHandle)
    .u64(value.bufferRevision)
    .bytesU32(encodeFsPath(value.path))
    .u64(value.byteLength)
    .bytes(value.contentHash)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeLspBufferRecord(bytes: Uint8Array): YasLspBufferRecord {
  const cursor = new YasCursor(bytes);
  const value = {
    workspaceHandle: cursor.u64("LSP workspace handle"),
    bufferHandle: cursor.u64("LSP buffer handle"),
    bufferRevision: cursor.u64("LSP buffer revision"),
    path: decodeFsPath(cursor.bytesU32("LSP buffer path")),
    byteLength: cursor.u64("LSP buffer byte length"),
    contentHash: new Uint8Array(cursor.take(32, "LSP buffer content hash")),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "LSP buffer state extensions",
    ),
  };
  cursor.end("LSP buffer state");
  encodeLspBufferRecord(value);
  return value;
}

export function encodeLspStateEntity(value: YasLspStateEntity): Uint8Array {
  const entityKind = lspEntityKind(value);
  const body =
    value.kind === "backend"
      ? encodeLspServerRecord(value.value)
      : value.kind === "diagnostics"
        ? encodeLspDiagnosticRecord(value.value)
        : encodeLspBufferRecord(value.value);
  return new YasWriter().u16(entityKind).u16(0).bytesU32(body).finish();
}

export function decodeLspStateEntity(bytes: Uint8Array): YasLspStateEntity {
  const cursor = new YasCursor(bytes);
  const entityKind = cursor.u16("LSP entity kind");
  if (cursor.u16("LSP entity reserved") !== 0)
    throw new YasProtocolError("LSP entity reserved field is nonzero");
  const body = cursor.bytesU32("LSP entity body");
  let value: YasLspStateEntity;
  if (entityKind === g.YAS_LSP_ENTITY_BACKEND)
    value = { kind: "backend", value: decodeLspServerRecord(body) };
  else if (entityKind === g.YAS_LSP_ENTITY_DIAGNOSTICS)
    value = { kind: "diagnostics", value: decodeLspDiagnosticRecord(body) };
  else if (entityKind === g.YAS_LSP_ENTITY_BUFFER)
    value = { kind: "buffer", value: decodeLspBufferRecord(body) };
  else throw new YasProtocolError("unknown LSP state entity kind");
  cursor.end("LSP state entity");
  return value;
}

export function encodeLspEntityPatch(value: YasLspEntityPatch): Uint8Array {
  if (
    value.entityKind !== lspEntityKind(value.replacement) ||
    value.observedRevision === 0n
  )
    throw new YasProtocolError("invalid LSP state patch");
  rejectRequiredExtensions(value.extensions, "LSP state patch");
  return new YasWriter()
    .u16(value.entityKind)
    .u16(0)
    .u64(value.observedRevision)
    .bytesU32(encodeLspStateEntity(value.replacement))
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeLspEntityPatch(bytes: Uint8Array): YasLspEntityPatch {
  const cursor = new YasCursor(bytes);
  const entityKind = cursor.u16("LSP entity kind");
  if (cursor.u16("LSP patch reserved") !== 0)
    throw new YasProtocolError("LSP patch reserved field is nonzero");
  const value = {
    entityKind,
    observedRevision: cursor.u64("LSP observed revision"),
    replacement: decodeLspStateEntity(cursor.bytesU32("LSP replacement")),
    extensions: decodeExtensions(cursor, new Set(), "LSP patch extensions"),
  };
  cursor.end("LSP state patch");
  encodeLspEntityPatch(value);
  return value;
}

export function encodeLspRemovedEntity(value: YasLspRemovedEntity): Uint8Array {
  if (
    value.entityKind > g.YAS_LSP_ENTITY_BUFFER ||
    value.key.length === 0 ||
    value.key.length > g.YAS_LSP_MAX_ENTITY_KEY_BYTES ||
    value.removedRevision === 0n
  )
    throw new YasProtocolError("invalid removed LSP entity");
  return new YasWriter()
    .u16(value.entityKind)
    .u16(0)
    .bytesU32(value.key)
    .u64(value.removedRevision)
    .finish();
}

export function decodeLspRemovedEntity(bytes: Uint8Array): YasLspRemovedEntity {
  const cursor = new YasCursor(bytes);
  const entityKind = cursor.u16("LSP entity kind");
  if (cursor.u16("LSP remove reserved") !== 0)
    throw new YasProtocolError("LSP remove reserved field is nonzero");
  const value = {
    entityKind,
    key: new Uint8Array(cursor.bytesU32("LSP entity key")),
    removedRevision: cursor.u64("LSP removed revision"),
  };
  cursor.end("LSP removed entity");
  encodeLspRemovedEntity(value);
  return value;
}

export interface YasLspBufferUpload {
  stagingHandle: bigint;
  transfer: YasTransfer;
  extensions: readonly YasExtension[];
}

interface YasLspCatalogLimits {
  maxServers: number;
  maxBuffers: number;
  maxDiagnosticsPerFile: number;
}

export class YasLspCatalog {
  private current = new Map<string, YasLspStateEntity>();
  private currentRetention: YasStateCatalogueRetention<string>;
  private staging: Map<string, YasLspStateEntity> | null = null;
  private stagingRetention: YasStateCatalogueRetention<string> | null = null;
  private subscription: YasStateSubscription | null = null;
  private revision = 0n;
  private listeners = new Set<(snapshot: YasLspSnapshot) => void>();
  private snapshotRejectors = new Set<(error: Error) => void>();
  private removeInvalidation: (() => void) | null;
  private watchPromise: Promise<void> | null = null;
  private cancelPendingWatch: ((error: Error) => void) | null = null;
  private generation = 0;
  private disposed = false;

  constructor(
    private readonly connection: YasConnection,
    readonly workspaceHandle: bigint,
    private readonly limits: () => YasLspCatalogLimits = () => ({
      maxServers: g.YAS_LSP_MAX_SERVERS,
      maxBuffers: g.YAS_LSP_MAX_BUFFERS_PER_WORKSPACE,
      maxDiagnosticsPerFile: g.YAS_LSP_MAX_DIAGNOSTICS_PER_FILE,
    }),
  ) {
    this.currentRetention =
      YasStateCatalogueRetention.forConnection(connection);
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family === undefined || family === g.YAS_FAMILY_LSP)
        this.invalidateLocal();
    });
  }

  get snapshot(): YasLspSnapshot {
    const backends: YasLspServerRecord[] = [];
    const diagnostics: YasLspDiagnosticRecord[] = [];
    const buffers: YasLspBufferRecord[] = [];
    for (const entity of this.current.values()) {
      if (entity.kind === "backend") backends.push(entity.value);
      else if (entity.kind === "diagnostics") diagnostics.push(entity.value);
      else buffers.push(entity.value);
    }
    return { revision: this.revision, backends, diagnostics, buffers };
  }

  subscribe(listener: (snapshot: YasLspSnapshot) => void): () => void {
    this.assertOpen();
    this.listeners.add(listener);
    invokeLifecycleListener(listener, this.snapshot, "LSP catalogue");
    return () => this.listeners.delete(listener);
  }

  async firstSnapshot(
    options: YasWatchOptions & { datasets?: number } = {},
  ): Promise<YasLspSnapshot> {
    this.assertOpen();
    if (this.revision !== 0n && this.subscription?.active) return this.snapshot;
    let remove: (() => void) | undefined;
    let rejectLifecycle!: (error: Error) => void;
    const result = new Promise<YasLspSnapshot>((resolve) => {
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
    options: YasWatchOptions & { datasets?: number } = {},
  ): Promise<void> {
    this.assertOpen();
    if (this.subscription?.active) return;
    if (this.watchPromise) return this.watchPromise;
    this.clearState();
    const generation = this.generation;
    const operation = YasStateSubscription.watch(
      this.connection,
      g.YAS_FAMILY_LSP,
      g.YAS_LSP_WATCH,
      g.YAS_LSP_UNWATCH,
      g.YAS_LSP_STATE,
      g.YAS_LSP_STATE_ACK,
      options,
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
        encodeLspWatch(
          this.workspaceHandle,
          options.datasets ?? g.YAS_LSP_WATCH_DATASETS,
          statePayload,
        ),
    ).then(async (subscription) => {
      if (this.disposed || generation !== this.generation) {
        await subscription.unwatch().catch(() => undefined);
        throw new YasProtocolError(
          "LSP catalogue changed while WATCH was pending",
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
    this.cancelWatch("LSP catalogue WATCH was cancelled");
    this.cancelSnapshots("LSP catalogue snapshot wait was cancelled");
    this.generation++;
    const subscription = this.subscription;
    this.subscription = null;
    this.clearState();
    await subscription?.unwatch();
  }

  async dispose(): Promise<void> {
    if (this.disposed) return;
    this.disposed = true;
    this.cancelWatch("LSP catalogue was closed while WATCH was pending");
    this.cancelSnapshots("LSP catalogue closed before its first snapshot");
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
        throw new YasProtocolError("LSP snapshot records without begin");
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
        throw new YasProtocolError("LSP snapshot end without begin");
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
      let next: Map<string, YasLspStateEntity>;
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

  private validateCatalog(
    entities: ReadonlyMap<string, YasLspStateEntity>,
  ): void {
    const limits = this.limits();
    let servers = 0;
    let buffers = 0;
    for (const entity of entities.values()) {
      if (entity.kind === "backend") servers++;
      else if (entity.kind === "buffer") buffers++;
      else if (entity.value.diagnostics.length > limits.maxDiagnosticsPerFile)
        throw new YasProtocolError(
          "LSP diagnostics exceed negotiated per-file limit",
        );
    }
    if (servers > limits.maxServers)
      throw new YasProtocolError(
        "LSP catalogue exceeds negotiated server limit",
      );
    if (buffers > limits.maxBuffers)
      throw new YasProtocolError(
        "LSP catalogue exceeds negotiated buffer limit",
      );
  }

  private applyRecords(
    target: Map<string, YasLspStateEntity>,
    retention: YasStateCatalogueRetention<string>,
    records: readonly YasTypedRecord[],
  ): void {
    const originals = new Map<string, YasLspStateEntity | null>();
    const remember = (key: string) => {
      if (!originals.has(key)) originals.set(key, target.get(key) ?? null);
    };
    const replace = (key: string, decoded: YasLspStateEntity) => {
      const encoded = encodeLspStateEntity(decoded);
      const entity = decodeLspStateEntity(encoded);
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
          const entity = decodeLspStateEntity(record.body);
          const key = stateEntityKey(entity);
          const exists = target.has(key);
          if ((record.kind === YAS_STATE_ADD) === exists)
            throw new YasProtocolError("LSP ADD/REPLACE precondition failed");
          replace(key, entity);
        } else if (record.kind === YAS_STATE_PATCH) {
          const patch = decodeLspEntityPatch(record.body);
          const key = stateEntityKey(patch.replacement);
          const previous = target.get(key);
          if (
            !previous ||
            stateEntityRevision(previous) !== patch.observedRevision
          )
            throw new YasProtocolError("LSP PATCH precondition failed");
          replace(key, patch.replacement);
        } else if (record.kind === YAS_STATE_REMOVE) {
          const removed = decodeLspRemovedEntity(record.body);
          const key = `${removed.entityKind}:${hex(removed.key)}`;
          const previous = target.get(key);
          if (!previous)
            throw new YasProtocolError("LSP REMOVE names an unknown entity");
          remember(key);
          retention.remove(key);
          target.delete(key);
        } else throw new YasProtocolError("unsupported LSP state record kind");
      }
      this.validateCatalog(target);
    } catch (error) {
      for (const key of originals.keys()) retention.remove(key);
      for (const [key, original] of originals) {
        if (original) {
          retention.upsert(
            key,
            Math.max(
              encodeLspStateEntity(original).length,
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
    this.cancelWatch("LSP catalogue was invalidated while WATCH was pending");
    this.cancelSnapshots("LSP catalogue invalidated before its first snapshot");
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
      invokeLifecycleListener(listener, snapshot, "LSP catalogue");
  }

  private assertOpen(): void {
    if (this.disposed) throw new YasProtocolError("LSP catalogue is closed");
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

export class YasLspClient {
  private readonly transfers;
  private readonly workspaces = new Map<bigint, YasLspWorkspace>();
  private removeClosedEvent: (() => void) | null;
  private removeInvalidation: (() => void) | null;
  private generation = 0;
  private disposed = false;

  constructor(readonly connection: YasConnection) {
    connection.family(g.YAS_FAMILY_LSP, g.YAS_LSP_VERSION);
    this.transfers = transfersFor(connection);
    this.removeClosedEvent = connection.onEvent(
      g.YAS_FAMILY_LSP,
      g.YAS_LSP_CLOSED,
      ({ payload }) => {
        const closed = decodeLspClosed(payload);
        this.workspaces.get(closed.workspaceHandle)?.markClosed(closed);
        this.workspaces.delete(closed.workspaceHandle);
      },
    );
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family !== undefined && family !== g.YAS_FAMILY_LSP) return;
      this.generation++;
      for (const workspace of this.workspaces.values()) workspace.invalidate();
      this.workspaces.clear();
    });
  }

  async open(value: YasLspOpen): Promise<YasLspWorkspace> {
    this.assertOpen();
    const generation = this.generation;
    const opened = await this.connection.requestDecoded(
      g.YAS_FAMILY_LSP,
      g.YAS_LSP_OPEN,
      encodeLspOpen(value),
      decodeLspOpenResult,
    );
    const workspace = new YasLspWorkspace(this, opened);
    if (this.disposed || generation !== this.generation) {
      await workspace.close().catch(() => undefined);
      throw new YasProtocolError("LSP client changed while OPEN was pending");
    }
    this.workspaces.set(opened.workspaceHandle, workspace);
    return workspace;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.generation++;
    this.removeClosedEvent?.();
    this.removeClosedEvent = null;
    this.removeInvalidation?.();
    this.removeInvalidation = null;
    for (const workspace of [...this.workspaces.values()])
      void workspace.close().catch(() => undefined);
    this.workspaces.clear();
  }

  listServers(
    workspaceHandle = 0n,
    extensions: readonly YasExtension[] = [],
  ): Promise<readonly YasLspServerRecord[]> {
    this.assertOpen();
    return this.connection.requestDecoded(
      g.YAS_FAMILY_LSP,
      g.YAS_LSP_LIST_SERVERS,
      encodeLspListServers(workspaceHandle, extensions),
      decodeLspServerList,
    );
  }

  stopServer(value: YasLspStopServer): Promise<Uint8Array> {
    this.assertOpen();
    return this.connection.request(
      g.YAS_FAMILY_LSP,
      g.YAS_LSP_STOP_SERVER,
      encodeLspStopServer(value),
    );
  }

  transferManager() {
    return this.transfers;
  }

  releaseWorkspace(handle: bigint): void {
    this.workspaces.delete(handle);
  }

  private assertOpen(): void {
    if (this.disposed) throw new YasProtocolError("LSP client is closed");
  }
}

export class YasLspWorkspace {
  readonly catalog: YasLspCatalog;
  private closed = false;
  private closeEvent: YasLspClosed | null = null;
  private readonly closeListeners = new Set<(event: YasLspClosed) => void>();

  constructor(
    readonly client: YasLspClient,
    readonly opened: YasLspOpenResult,
  ) {
    this.catalog = new YasLspCatalog(
      client.connection,
      opened.workspaceHandle,
      () => ({
        maxServers: negotiatedStateLimitU32(
          client.connection,
          g.YAS_FAMILY_LSP,
          g.YAS_LSP_VERSION,
          g.YAS_LSP_LIMIT_MAX_SERVERS,
          g.YAS_LSP_MAX_SERVERS,
        ),
        maxBuffers: negotiatedStateLimitU32(
          client.connection,
          g.YAS_FAMILY_LSP,
          g.YAS_LSP_VERSION,
          g.YAS_LSP_LIMIT_MAX_BUFFERS_PER_WORKSPACE,
          g.YAS_LSP_MAX_BUFFERS_PER_WORKSPACE,
        ),
        maxDiagnosticsPerFile: negotiatedStateLimitU32(
          client.connection,
          g.YAS_FAMILY_LSP,
          g.YAS_LSP_VERSION,
          g.YAS_LSP_LIMIT_MAX_DIAGNOSTICS_PER_FILE,
          g.YAS_LSP_MAX_DIAGNOSTICS_PER_FILE,
        ),
      }),
    );
  }

  get handle(): bigint {
    return this.opened.workspaceHandle;
  }

  list(
    options: YasWatchOptions & { datasets?: number } = {},
  ): Promise<YasLspSnapshot> {
    this.assertOpen();
    return this.catalog.firstSnapshot(options);
  }

  onClosed(listener: (event: YasLspClosed) => void): () => void {
    this.closeListeners.add(listener);
    if (this.closeEvent)
      invokeLifecycleListener(listener, this.closeEvent, "LSP close");
    return () => this.closeListeners.delete(listener);
  }

  async close(extensions: readonly YasExtension[] = []): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.client.releaseWorkspace(this.handle);
    const event = {
      workspaceHandle: this.handle,
      reason: g.YAS_LSP_CLOSED_CLIENT_REQUEST,
      detail: "",
    };
    this.finishClosed(event, false);
    await this.catalog.dispose().catch(() => undefined);
    await this.client.connection.request(
      g.YAS_FAMILY_LSP,
      g.YAS_LSP_CLOSE,
      encodeLspClose(this.handle, extensions),
    );
  }

  async query(
    body: YasLspQueryBody,
    options: {
      maxRecords?: number;
      cursor?: Uint8Array;
      initialReceiveCredit?: bigint;
      extensions?: readonly YasExtension[];
    } = {},
  ): Promise<YasLspQueryPage> {
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
        g.YAS_FAMILY_LSP,
        g.YAS_LSP_QUERY,
        encodeLspQuery({
          workspaceHandle: this.handle,
          maxRecords: options.maxRecords ?? 256,
          cursor: options.cursor ?? new Uint8Array(),
          initialReceiveCredit: lease.bytes,
          body,
          extensions: options.extensions,
        }),
        (payload) => {
          const page = decodeLspQueryPage(payload);
          if (page.delivery.kind === "inline") {
            lease.release();
            released = true;
            const records = page.delivery.records.map(cloneQueryRecord);
            return {
              queryStatus: page.queryStatus,
              flags: page.flags,
              detail: page.detail,
              nextCursor: new Uint8Array(page.nextCursor),
              totalHint: page.totalHint,
              records: () => Promise.resolve(records.map(cloneQueryRecord)),
            };
          }
          const transfer = manager.acceptServerDescriptor(
            page.delivery.descriptor,
            lease,
          );
          accepted = true;
          const records = collectLspQueryRecords(transfer);
          return {
            queryStatus: page.queryStatus,
            flags: page.flags,
            detail: page.detail,
            nextCursor: new Uint8Array(page.nextCursor),
            totalHint: page.totalHint,
            records: () => records,
          };
        },
      );
    } catch (error) {
      if (!accepted && !released) lease.release();
      throw error;
    }
  }

  bufferPut(
    value: Omit<YasLspBufferPut, "workspaceHandle">,
  ): Promise<YasLspBufferIdentity> {
    this.assertOpen();
    return this.client.connection.requestDecoded(
      g.YAS_FAMILY_LSP,
      g.YAS_LSP_BUFFER_PUT,
      encodeLspBufferPut({ ...value, workspaceHandle: this.handle }),
      decodeLspBufferIdentity,
    );
  }

  async bufferBegin(
    value: Omit<YasLspBufferBegin, "workspaceHandle">,
  ): Promise<YasLspBufferUpload> {
    this.assertOpen();
    const result = await this.client.connection.requestDecoded(
      g.YAS_FAMILY_LSP,
      g.YAS_LSP_BUFFER_BEGIN,
      encodeLspBufferBegin({ ...value, workspaceHandle: this.handle }),
      decodeLspBufferBeginResult,
    );
    return {
      stagingHandle: result.stagingHandle,
      transfer: this.client
        .transferManager()
        .acceptServerUploadDescriptor(result.descriptor),
      extensions: result.extensions,
    };
  }

  bufferCommit(value: YasLspBufferCommit): Promise<YasLspBufferIdentity> {
    this.assertOpen();
    return this.client.connection.requestDecoded(
      g.YAS_FAMILY_LSP,
      g.YAS_LSP_BUFFER_COMMIT,
      encodeLspBufferCommit(value),
      decodeLspBufferIdentity,
    );
  }

  bufferClose(value: YasLspBufferClose): Promise<Uint8Array> {
    this.assertOpen();
    return this.client.connection.request(
      g.YAS_FAMILY_LSP,
      g.YAS_LSP_BUFFER_CLOSE,
      encodeLspBufferClose(value),
    );
  }

  private assertOpen(): void {
    if (this.closed) throw new YasProtocolError("LSP workspace is closed");
  }

  markClosed(event: YasLspClosed): void {
    this.finishClosed(event);
  }

  invalidate(): void {
    if (this.closed) return;
    this.markClosed({
      workspaceHandle: this.handle,
      reason: g.YAS_LSP_CLOSED_BACKEND_FAILED,
      detail: "LSP session invalidated",
    });
  }

  private finishClosed(event: YasLspClosed, disposeCatalog = true): void {
    if (this.closeEvent) return;
    this.closed = true;
    this.closeEvent = event;
    this.client.releaseWorkspace(this.handle);
    if (disposeCatalog) void this.catalog.dispose().catch(() => undefined);
    for (const listener of this.closeListeners)
      invokeLifecycleListener(listener, event, "LSP close");
    this.closeListeners.clear();
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

async function collectLspQueryRecords(
  transfer: YasTransfer,
): Promise<readonly YasLspQueryRecord[]> {
  const records: YasLspQueryRecord[] = [];
  let byteLength = 0;
  try {
    while (true) {
      const message = await transfer.readMessage();
      if (message === null) break;
      byteLength += message.length;
      if (byteLength > g.YAS_LSP_MAX_QUERY_BYTES)
        throw new YasProtocolError("LSP query Transfer exceeds byte limit");
      const cursor = new YasCursor(message);
      while (cursor.remaining !== 0) {
        const record = decodeLspQueryRecord(cursor);
        if (record) records.push(record);
        if (records.length > g.YAS_LSP_MAX_QUERY_RECORDS)
          throw new YasProtocolError("LSP query Transfer exceeds record limit");
      }
    }
    return records;
  } catch (error) {
    transfer.reset();
    throw error;
  }
}

function decodePosition(cursor: YasCursor): YasLspPosition {
  return {
    line: cursor.u32("LSP line"),
    byteColumn: cursor.u32("LSP UTF-8 byte column"),
  };
}

function decodeRange(cursor: YasCursor): YasLspTextRange {
  const value = { start: decodePosition(cursor), end: decodePosition(cursor) };
  validateRange(value);
  return value;
}

function decodeDocumentTarget(cursor: YasCursor): YasLspDocumentTarget {
  const value = {
    path: decodeFsPath(cursor.bytesU32("LSP document path")),
    documentRevision: cursor.u64("LSP document revision"),
    contentHash: new Uint8Array(cursor.take(32, "LSP document content hash")),
  };
  requireDocumentPath(value.path);
  return value;
}

function decodeEdit(cursor: YasCursor): YasLspEditRecord {
  const value: YasLspEditRecord = {
    kind: "edit",
    path: decodeFsPath(cursor.bytesU32("LSP edit path")),
    expectedRevision: cursor.u64("LSP edit expected revision"),
    expectedContentHash: new Uint8Array(
      cursor.take(32, "LSP edit expected content hash"),
    ),
    range: decodeRange(cursor),
    replacement: new Uint8Array(cursor.bytesU32("LSP edit replacement")),
  };
  encodeLspEditRecord(value);
  return value;
}

function validatePosition(value: YasLspPosition): void {
  if (
    !Number.isInteger(value.line) ||
    value.line < 0 ||
    value.line > 0xffff_ffff ||
    !Number.isInteger(value.byteColumn) ||
    value.byteColumn < 0 ||
    value.byteColumn > 0xffff_ffff
  )
    throw new YasProtocolError("invalid LSP position");
}

function validateRange(value: YasLspTextRange): void {
  validatePosition(value.start);
  validatePosition(value.end);
  if (
    value.start.line > value.end.line ||
    (value.start.line === value.end.line &&
      value.start.byteColumn > value.end.byteColumn)
  )
    throw new YasProtocolError("reversed LSP text range");
}

function requireDocumentPath(path: YasFsPath): void {
  if (path.components.length === 0)
    throw new YasProtocolError("empty LSP document path");
  encodeFsPath(path);
}

function validatePlatformPath(value: Uint8Array, field: string): void {
  if (
    value.length === 0 ||
    value.length > g.YAS_LSP_MAX_ROOT_BYTES ||
    value.includes(0)
  )
    throw new YasProtocolError(`invalid ${field}`);
}

function requireHandle(value: bigint, field: string): void {
  if (value === 0n) throw new YasProtocolError(`zero ${field}`);
}

function requireRevision(value: bigint, field: string): void {
  if (value === 0n) throw new YasProtocolError(`zero ${field}`);
}

function requireOperationId(value: Uint8Array): void {
  if (value.length !== 16 || value.every((byte) => byte === 0))
    throw new YasProtocolError("invalid LSP operation ID");
}

function requireHash(value: Uint8Array): void {
  if (value.length !== 32)
    throw new YasProtocolError("LSP content hash must contain 32 bytes");
}

function requireName(value: string, maximum: number, field: string): void {
  const length = utf8Length(value);
  if (length === 0 || length > maximum || value.includes("\0"))
    throw new YasProtocolError(`invalid ${field}`);
}

function rejectRequiredExtensions(
  extensions: readonly YasExtension[] | undefined,
  context: string,
): void {
  encodeExtensions(extensions);
  if (extensions?.some((extension) => extension.required))
    throw new YasProtocolError(`unknown required ${context} extension`);
}

function requireZero(value: Uint8Array, field: string): void {
  if (value.some((byte) => byte !== 0))
    throw new YasProtocolError(`${field} reserved bytes are nonzero`);
}

function validateBufferIdentity(value: YasLspBufferIdentity): void {
  requireHandle(value.bufferHandle, "LSP buffer handle");
  requireRevision(value.bufferRevision, "LSP buffer revision");
  requireRevision(value.workspaceRevision, "LSP workspace revision");
  requireHash(value.contentHash);
  if (value.byteLength > BigInt(g.YAS_LSP_MAX_BUFFER_BYTES))
    throw new YasProtocolError("LSP buffer exceeds limit");
  rejectRequiredExtensions(value.extensions, "LSP buffer identity");
}

function validateBufferDescriptor(value: YasTransferDescriptor): void {
  if (
    value.mode !== YAS_TRANSFER_MODE_BYTE ||
    value.direction !== YAS_TRANSFER_RECEIVER_TO_SENDER ||
    value.receiverSendCredit === 0n ||
    value.senderSendCredit !== 0n ||
    value.maxItemBytes !== 0n ||
    value.contentFamily !== g.YAS_FAMILY_LSP ||
    value.contentKind !== g.YAS_LSP_BUFFER_CONTENT_KIND ||
    value.contentVersion !== g.YAS_LSP_VERSION ||
    value.sensitiveContent !== true
  )
    throw new YasProtocolError("invalid LSP buffer Transfer descriptor");
}

function validateQueryDescriptor(value: YasTransferDescriptor): void {
  if (
    value.mode !== YAS_TRANSFER_MODE_MESSAGE ||
    value.direction !== YAS_TRANSFER_SENDER_TO_RECEIVER ||
    value.contentFamily !== g.YAS_FAMILY_LSP ||
    value.contentKind !== g.YAS_LSP_QUERY_CONTENT_KIND ||
    value.contentVersion !== g.YAS_LSP_VERSION ||
    value.sensitiveContent !== true
  )
    throw new YasProtocolError("invalid LSP query Transfer descriptor");
}

function validateServer(
  value: YasLspServerRecord,
  allowDetachedWorkspace = false,
): void {
  requireHandle(value.serverHandle, "LSP server handle");
  requireRevision(value.generation, "LSP server generation");
  requireRevision(value.serverRevision, "LSP server revision");
  if (value.workspaceHandle !== 0n || !allowDetachedWorkspace)
    requireHandle(value.workspaceHandle, "LSP workspace handle");
  if (
    value.phase > g.YAS_LSP_SERVER_FAILED ||
    (value.progressPercent > 100 &&
      value.progressPercent !== g.YAS_LSP_SERVER_PROGRESS_UNKNOWN) ||
    value.capabilities & ~BigInt(g.YAS_LSP_CAPABILITIES) ||
    utf8Length(value.lastMessage) > g.YAS_LSP_MAX_DETAIL_BYTES
  )
    throw new YasProtocolError("invalid LSP server metadata");
  requireName(value.language, g.YAS_LSP_MAX_LANGUAGE_BYTES, "LSP language");
  requireName(value.profile, g.YAS_LSP_MAX_PROFILE_BYTES, "LSP profile");
  requireName(
    value.backendId,
    g.YAS_LSP_MAX_BACKEND_ID_BYTES,
    "LSP backend ID",
  );
  rejectRequiredExtensions(value.extensions, "LSP server");
}

function validateDiagnostic(value: YasLspDiagnostic): void {
  requireHandle(value.diagnosticId, "LSP diagnostic ID");
  validateRange(value.range);
  if (
    value.severity > g.YAS_LSP_DIAGNOSTIC_HINT ||
    value.tags & ~g.YAS_LSP_DIAGNOSTIC_TAGS ||
    utf8Length(value.code) > g.YAS_LSP_MAX_DIAGNOSTIC_CODE_BYTES ||
    utf8Length(value.source) > g.YAS_LSP_MAX_LANGUAGE_BYTES ||
    utf8Length(value.message) > g.YAS_LSP_MAX_DIAGNOSTIC_MESSAGE_BYTES
  )
    throw new YasProtocolError("invalid LSP diagnostic");
}

function lspEntityKind(value: YasLspStateEntity): number {
  if (value.kind === "backend") return g.YAS_LSP_ENTITY_BACKEND;
  if (value.kind === "diagnostics") return g.YAS_LSP_ENTITY_DIAGNOSTICS;
  return g.YAS_LSP_ENTITY_BUFFER;
}

function stateEntityKey(value: YasLspStateEntity): string {
  const writer = new YasWriter();
  if (value.kind === "backend") writer.u64(value.value.serverHandle);
  else if (value.kind === "diagnostics")
    writer.bytes(encodeFsPath(value.value.path));
  else writer.u64(value.value.bufferHandle);
  return `${lspEntityKind(value)}:${hex(writer.finish())}`;
}

function stateEntityRevision(value: YasLspStateEntity): bigint {
  if (value.kind === "backend") return value.value.serverRevision;
  if (value.kind === "diagnostics") return value.value.diagnosticsRevision;
  return value.value.bufferRevision;
}

function cloneQueryRecord(value: YasLspQueryRecord): YasLspQueryRecord {
  // Round-tripping gives callers isolated byte arrays without a large bespoke
  // deep-clone surface and revalidates the typed record.
  const cursor = new YasCursor(encodeLspQueryRecord(value));
  const clone = decodeLspQueryRecord(cursor);
  cursor.end("LSP cloned query record");
  if (!clone) throw new YasProtocolError("known LSP record was skipped");
  return clone;
}

function hex(value: Uint8Array): string {
  return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join(
    "",
  );
}

function utf8Length(value: string): number {
  return encoder.encode(value).length;
}
