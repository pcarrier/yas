/** YAS persistent key/value family codecs and client primitives. */

import * as g from "./generated";
import type { YasConnection } from "./session";
import {
  YAS_STATE_ADD,
  YAS_STATE_PATCH,
  YAS_STATE_REMOVE,
  YAS_STATE_REPLACE,
  YasStateSubscription,
  encodeWatch,
  type YasStateBatch,
  type YasWatchOptions,
} from "./state";
import {
  decodeInlineOrTransfer,
  decodeTransferDescriptor,
  encodeInlineOrTransfer,
  encodeTransferDescriptor,
  requireTransferUploadStage,
  transfersFor,
  type YasInlineOrTransfer,
  type YasTransferDescriptor,
} from "./transfer";
import {
  YasCursor,
  YasProtocolError,
  YasResultError,
  YasWriter,
  decodeExtensions,
  encodeExtensions,
  equalBytes,
  type YasExtension,
} from "./wire";

export interface YasKvOpen {
  prefix: Uint8Array;
  extensions?: readonly YasExtension[];
}

export interface YasKvOpenResult {
  namespaceHandle: bigint;
  storeRevision: bigint;
  extensions: readonly YasExtension[];
}

export interface YasKvWatch {
  namespaceHandle: bigint;
  inlineMax: number;
  options?: YasWatchOptions;
  initialCredit: bigint;
}

export interface YasKvEntry {
  relativeKey: Uint8Array;
  contentHash: Uint8Array;
  byteLength: bigint;
  modificationRevision: bigint;
  modifiedUnixNs: bigint;
  inlineValue?: Uint8Array;
  extensions: readonly YasExtension[];
}

export interface YasKvGet {
  namespaceHandle: bigint;
  relativeKey: Uint8Array;
  initialReceiveCredit: bigint;
  extensions?: readonly YasExtension[];
}

export interface YasKvGetResult {
  modificationRevision: bigint;
  value: YasInlineOrTransfer;
}

export interface YasKvStageValue {
  byteLength: bigint;
  contentHash: Uint8Array;
  extensions?: readonly YasExtension[];
}

export interface YasKvStageValueResult {
  stagingHandle: bigint;
  byteLength: bigint;
  contentHash: Uint8Array;
  descriptor: YasTransferDescriptor;
}

export type YasKvPrecondition =
  | { type: "any" }
  | { type: "absent" }
  | { type: "hash"; contentHash: Uint8Array }
  | { type: "revision"; modificationRevision: bigint }
  | {
      type: "hash-and-revision";
      contentHash: Uint8Array;
      modificationRevision: bigint;
    };

export type YasKvValueSource =
  | { type: "inline"; bytes: Uint8Array }
  | { type: "staged"; stagingHandle: bigint };

export interface YasKvPut {
  namespaceHandle: bigint;
  operationId: Uint8Array;
  durable?: boolean;
  relativeKey: Uint8Array;
  precondition: YasKvPrecondition;
  value: YasKvValueSource;
  extensions?: readonly YasExtension[];
}

export interface YasKvDelete {
  namespaceHandle: bigint;
  operationId: Uint8Array;
  durable?: boolean;
  relativeKey: Uint8Array;
  precondition: YasKvPrecondition;
  extensions?: readonly YasExtension[];
}

export type YasKvMutation =
  | {
      type: "put";
      relativeKey: Uint8Array;
      precondition: YasKvPrecondition;
      value: YasKvValueSource;
      extensions?: readonly YasExtension[];
    }
  | {
      type: "delete";
      relativeKey: Uint8Array;
      precondition: YasKvPrecondition;
      extensions?: readonly YasExtension[];
    };

export interface YasKvBatch {
  namespaceHandle: bigint;
  operationId: Uint8Array;
  durable?: boolean;
  mutations: readonly YasKvMutation[];
  extensions?: readonly YasExtension[];
}

export interface YasKvMutationResult {
  status: number;
  modificationRevision: bigint;
  modifiedUnixNs: bigint;
  contentHash: Uint8Array;
  byteLength: bigint;
  extensions: readonly YasExtension[];
}

export interface YasKvBatchResult {
  storeRevision: bigint;
  results: readonly YasKvMutationResult[];
  extensions: readonly YasExtension[];
}

export function encodeKvOpen(value: YasKvOpen): Uint8Array {
  validatePrefix(value.prefix);
  validateNoRequired(value.extensions, "KV OPEN");
  return new YasWriter()
    .bytesU16(value.prefix)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeKvOpen(bytes: Uint8Array): Required<YasKvOpen> {
  const cursor = new YasCursor(bytes);
  const prefix = new Uint8Array(cursor.bytesU16("KV namespace prefix"));
  const extensions = noRequiredExtensions(cursor, "KV OPEN extensions");
  cursor.end("KV OPEN");
  validatePrefix(prefix);
  return { prefix, extensions };
}

export function encodeKvOpenResult(value: YasKvOpenResult): Uint8Array {
  validateHandle(value.namespaceHandle, "KV namespace handle");
  validateRevision(value.storeRevision, "KV store revision");
  validateNoRequired(value.extensions, "KV OPEN Result");
  return new YasWriter()
    .u64(value.namespaceHandle)
    .u64(value.storeRevision)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeKvOpenResult(bytes: Uint8Array): YasKvOpenResult {
  const cursor = new YasCursor(bytes);
  const namespaceHandle = cursor.u64("KV namespace handle");
  const storeRevision = cursor.u64("KV store revision");
  const extensions = noRequiredExtensions(cursor, "KV OPEN Result extensions");
  cursor.end("KV OPEN Result");
  validateHandle(namespaceHandle, "KV namespace handle");
  validateRevision(storeRevision, "KV store revision");
  return { namespaceHandle, storeRevision, extensions };
}

export function encodeKvClose(
  namespaceHandle: bigint,
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  validateHandle(namespaceHandle, "KV namespace handle");
  validateNoRequired(extensions, "KV CLOSE");
  return new YasWriter()
    .u64(namespaceHandle)
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function encodeKvWatch(value: YasKvWatch): Uint8Array {
  validateHandle(value.namespaceHandle, "KV namespace handle");
  if (
    !Number.isInteger(value.inlineMax) ||
    value.inlineMax < 0 ||
    value.inlineMax > g.YAS_KV_MAX_INLINE_BYTES
  )
    throw new YasProtocolError("invalid KV WATCH inline limit");
  const state = encodeWatch(value.options ?? {}, value.initialCredit);
  return new YasWriter()
    .u64(value.namespaceHandle)
    .u32(value.inlineMax)
    .u32(0)
    .bytesU32(state)
    .finish();
}

export function decodeKvWatch(bytes: Uint8Array): YasKvWatch {
  const cursor = new YasCursor(bytes);
  const namespaceHandle = cursor.u64("KV namespace handle");
  const inlineMax = cursor.u32("KV WATCH inline limit");
  if (cursor.u32("KV WATCH reserved") !== 0)
    throw new YasProtocolError("KV WATCH reserved field is nonzero");
  const state = cursor.sub(cursor.u32("State WATCH length"), "State WATCH");
  const flags = state.u16("State WATCH flags");
  if (flags & ~g.YAS_STATE_WATCH_RESUME)
    throw new YasProtocolError("unknown State WATCH flags");
  if (state.u16("State WATCH reserved") !== 0)
    throw new YasProtocolError("State WATCH reserved field is nonzero");
  const initialCredit = state.u64("State WATCH initial credit");
  let resume: YasWatchOptions["resume"];
  if (flags & g.YAS_STATE_WATCH_RESUME) {
    const bootId = new Uint8Array(state.take(16, "State WATCH boot ID"));
    const revision = state.u64("State WATCH revision");
    validateRevision(revision, "State WATCH revision");
    resume = { bootId, revision };
  }
  const extensions = decodeExtensions(
    state,
    new Set(),
    "State WATCH extensions",
  );
  state.end("State WATCH");
  cursor.end("KV WATCH");
  const value = {
    namespaceHandle,
    inlineMax,
    initialCredit,
    options: { resume, extensions },
  };
  encodeKvWatch(value);
  return value;
}

export function encodeKvEntry(value: YasKvEntry): Uint8Array {
  validateRelativeKey(value.relativeKey);
  validateHash(value.contentHash);
  validateValueLength(value.byteLength);
  validateRevision(value.modificationRevision, "KV modification revision");
  validateNoRequired(value.extensions, "KV entry");
  if (
    value.inlineValue &&
    (value.inlineValue.length > g.YAS_KV_MAX_INLINE_BYTES ||
      BigInt(value.inlineValue.length) !== value.byteLength)
  )
    throw new YasProtocolError("invalid KV inline value length");
  const writer = new YasWriter()
    .bytesU16(value.relativeKey)
    .bytes(value.contentHash)
    .u64(value.byteLength)
    .u64(value.modificationRevision)
    .i64(value.modifiedUnixNs)
    .u8(
      value.inlineValue === undefined
        ? g.YAS_KV_CONTENT_NONE
        : g.YAS_KV_CONTENT_INLINE,
    )
    .bytes(new Uint8Array(3));
  if (value.inlineValue !== undefined) writer.bytesU32(value.inlineValue);
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeKvEntry(bytes: Uint8Array): YasKvEntry {
  const cursor = new YasCursor(bytes);
  const relativeKey = new Uint8Array(cursor.bytesU16("KV relative key"));
  const contentHash = new Uint8Array(cursor.take(32, "KV content hash"));
  const byteLength = cursor.u64("KV value length");
  const modificationRevision = cursor.u64("KV modification revision");
  const modifiedUnixNs = cursor.i64("KV modified time");
  const content = cursor.u8("KV entry content kind");
  reserved(cursor, 3, "KV entry");
  let inlineValue: Uint8Array | undefined;
  if (content === g.YAS_KV_CONTENT_INLINE)
    inlineValue = new Uint8Array(cursor.bytesU32("KV inline value"));
  else if (content !== g.YAS_KV_CONTENT_NONE)
    throw new YasProtocolError("unknown KV entry content kind");
  const extensions = noRequiredExtensions(cursor, "KV entry extensions");
  cursor.end("KV entry");
  const value = {
    relativeKey,
    contentHash,
    byteLength,
    modificationRevision,
    modifiedUnixNs,
    inlineValue,
    extensions,
  };
  encodeKvEntry(value);
  return value;
}

export function encodeKvRemovedEntry(
  relativeKey: Uint8Array,
  modificationRevision: bigint,
): Uint8Array {
  validateRelativeKey(relativeKey);
  validateRevision(modificationRevision, "KV modification revision");
  return new YasWriter()
    .bytesU16(relativeKey)
    .u64(modificationRevision)
    .finish();
}

export function decodeKvRemovedEntry(bytes: Uint8Array): {
  relativeKey: Uint8Array;
  modificationRevision: bigint;
} {
  const cursor = new YasCursor(bytes);
  const relativeKey = new Uint8Array(cursor.bytesU16("KV relative key"));
  const modificationRevision = cursor.u64("KV modification revision");
  cursor.end("KV removed entry");
  validateRelativeKey(relativeKey);
  validateRevision(modificationRevision, "KV modification revision");
  return { relativeKey, modificationRevision };
}

export function encodeKvGet(value: YasKvGet): Uint8Array {
  validateHandle(value.namespaceHandle, "KV namespace handle");
  validateRelativeKey(value.relativeKey);
  validateNoRequired(value.extensions, "KV GET");
  return new YasWriter()
    .u64(value.namespaceHandle)
    .bytesU16(value.relativeKey)
    .u64(value.initialReceiveCredit)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeKvGet(bytes: Uint8Array): Required<YasKvGet> {
  const cursor = new YasCursor(bytes);
  const namespaceHandle = cursor.u64("KV namespace handle");
  const relativeKey = new Uint8Array(cursor.bytesU16("KV relative key"));
  const initialReceiveCredit = cursor.u64("KV initial receive credit");
  const extensions = noRequiredExtensions(cursor, "KV GET extensions");
  cursor.end("KV GET");
  validateHandle(namespaceHandle, "KV namespace handle");
  validateRelativeKey(relativeKey);
  return { namespaceHandle, relativeKey, initialReceiveCredit, extensions };
}

export function encodeKvGetResult(value: YasKvGetResult): Uint8Array {
  validateRevision(value.modificationRevision, "KV modification revision");
  validateKvDelivery(value.value, "download");
  return new YasWriter()
    .u64(value.modificationRevision)
    .bytes(encodeInlineOrTransfer(value.value))
    .finish();
}

export function decodeKvGetResult(bytes: Uint8Array): YasKvGetResult {
  const cursor = new YasCursor(bytes);
  const modificationRevision = cursor.u64("KV modification revision");
  const value = decodeInlineOrTransfer(cursor.take(cursor.remaining));
  validateRevision(modificationRevision, "KV modification revision");
  validateKvDelivery(value, "download");
  return { modificationRevision, value };
}

export function encodeKvStageValue(value: YasKvStageValue): Uint8Array {
  validateValueLength(value.byteLength);
  validateHash(value.contentHash);
  validateNoRequired(value.extensions, "KV STAGE_VALUE");
  return new YasWriter()
    .u64(value.byteLength)
    .bytes(value.contentHash)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeKvStageValue(
  bytes: Uint8Array,
): Required<YasKvStageValue> {
  const cursor = new YasCursor(bytes);
  const byteLength = cursor.u64("KV staged value length");
  const contentHash = new Uint8Array(cursor.take(32, "KV staged value hash"));
  const extensions = noRequiredExtensions(cursor, "KV STAGE_VALUE extensions");
  cursor.end("KV STAGE_VALUE");
  validateValueLength(byteLength);
  return { byteLength, contentHash, extensions };
}

export function encodeKvStageValueResult(
  value: YasKvStageValueResult,
): Uint8Array {
  validateHandle(value.stagingHandle, "KV staging handle");
  validateValueLength(value.byteLength);
  validateHash(value.contentHash);
  validateKvDescriptor(value.descriptor, "upload");
  requireTransferUploadStage(
    value.descriptor,
    value.stagingHandle,
    "KV staged-value descriptor",
  );
  return new YasWriter()
    .u64(value.stagingHandle)
    .u64(value.byteLength)
    .bytes(value.contentHash)
    .bytes(encodeTransferDescriptor(value.descriptor))
    .finish();
}

export function decodeKvStageValueResult(
  bytes: Uint8Array,
): YasKvStageValueResult {
  const cursor = new YasCursor(bytes);
  const stagingHandle = cursor.u64("KV staging handle");
  const byteLength = cursor.u64("KV staged value length");
  const contentHash = new Uint8Array(cursor.take(32, "KV staged value hash"));
  const descriptor = decodeTransferDescriptor(cursor);
  cursor.end("KV STAGE_VALUE Result");
  const value = { stagingHandle, byteLength, contentHash, descriptor };
  encodeKvStageValueResult(value);
  return value;
}

export function encodeKvPut(value: YasKvPut): Uint8Array {
  validateMutationHeader(value);
  const writer = new YasWriter()
    .u64(value.namespaceHandle)
    .bytes(value.operationId)
    .u16(value.durable ? g.YAS_KV_MUTATION_DURABLE : 0)
    .u16(0)
    .bytesU16(value.relativeKey);
  encodePrecondition(writer, value.precondition);
  encodeValueSource(writer, value.value);
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeKvPut(bytes: Uint8Array): Required<YasKvPut> {
  const cursor = new YasCursor(bytes);
  const namespaceHandle = cursor.u64("KV namespace handle");
  const operationId = new Uint8Array(cursor.take(16, "KV operation ID"));
  const flags = cursor.u16("KV PUT flags");
  if (flags & ~g.YAS_KV_MUTATION_DURABLE)
    throw new YasProtocolError("unknown KV PUT flags");
  if (cursor.u16("KV PUT reserved") !== 0)
    throw new YasProtocolError("KV PUT reserved field is nonzero");
  const relativeKey = new Uint8Array(cursor.bytesU16("KV relative key"));
  const precondition = decodePrecondition(cursor);
  const value = decodeValueSource(cursor);
  const extensions = noRequiredExtensions(cursor, "KV PUT extensions");
  cursor.end("KV PUT");
  const result = {
    namespaceHandle,
    operationId,
    durable: flags !== 0,
    relativeKey,
    precondition,
    value,
    extensions,
  };
  encodeKvPut(result);
  return result;
}

export function encodeKvDelete(value: YasKvDelete): Uint8Array {
  validateMutationHeader(value);
  if (value.precondition.type === "absent")
    throw new YasProtocolError("delete-if-absent is invalid");
  const writer = new YasWriter()
    .u64(value.namespaceHandle)
    .bytes(value.operationId)
    .u16(value.durable ? g.YAS_KV_MUTATION_DURABLE : 0)
    .u16(0)
    .bytesU16(value.relativeKey);
  encodePrecondition(writer, value.precondition);
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeKvDelete(bytes: Uint8Array): Required<YasKvDelete> {
  const cursor = new YasCursor(bytes);
  const namespaceHandle = cursor.u64("KV namespace handle");
  const operationId = new Uint8Array(cursor.take(16, "KV operation ID"));
  const flags = cursor.u16("KV DELETE flags");
  if (flags & ~g.YAS_KV_MUTATION_DURABLE)
    throw new YasProtocolError("unknown KV DELETE flags");
  if (cursor.u16("KV DELETE reserved") !== 0)
    throw new YasProtocolError("KV DELETE reserved field is nonzero");
  const relativeKey = new Uint8Array(cursor.bytesU16("KV relative key"));
  const precondition = decodePrecondition(cursor);
  const extensions = noRequiredExtensions(cursor, "KV DELETE extensions");
  cursor.end("KV DELETE");
  const result = {
    namespaceHandle,
    operationId,
    durable: flags !== 0,
    relativeKey,
    precondition,
    extensions,
  };
  encodeKvDelete(result);
  return result;
}

export function encodeKvMutation(value: YasKvMutation): Uint8Array {
  validateRelativeKey(value.relativeKey);
  validateNoRequired(value.extensions, "KV mutation");
  if (value.type === "delete" && value.precondition.type === "absent")
    throw new YasProtocolError("delete-if-absent is invalid");
  const writer = new YasWriter()
    .u8(value.type === "put" ? g.YAS_KV_MUTATION_PUT : g.YAS_KV_MUTATION_DELETE)
    .bytes(new Uint8Array(3))
    .bytesU16(value.relativeKey);
  encodePrecondition(writer, value.precondition);
  if (value.type === "put") encodeValueSource(writer, value.value);
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeKvMutation(bytes: Uint8Array): YasKvMutation {
  const cursor = new YasCursor(bytes);
  const kind = cursor.u8("KV mutation kind");
  reserved(cursor, 3, "KV mutation");
  const relativeKey = new Uint8Array(cursor.bytesU16("KV relative key"));
  const precondition = decodePrecondition(cursor);
  let value: YasKvMutation;
  if (kind === g.YAS_KV_MUTATION_PUT) {
    value = {
      type: "put",
      relativeKey,
      precondition,
      value: decodeValueSource(cursor),
      extensions: noRequiredExtensions(cursor, "KV mutation extensions"),
    };
  } else if (kind === g.YAS_KV_MUTATION_DELETE) {
    value = {
      type: "delete",
      relativeKey,
      precondition,
      extensions: noRequiredExtensions(cursor, "KV mutation extensions"),
    };
  } else {
    throw new YasProtocolError("unknown KV mutation kind");
  }
  cursor.end("KV mutation");
  encodeKvMutation(value);
  return value;
}

export function encodeKvBatch(value: YasKvBatch): Uint8Array {
  validateHandle(value.namespaceHandle, "KV namespace handle");
  validateOperationId(value.operationId);
  if (
    value.mutations.length === 0 ||
    value.mutations.length > g.YAS_KV_MAX_BATCH_ITEMS
  )
    throw new YasProtocolError("invalid KV batch item count");
  validateNoRequired(value.extensions, "KV BATCH");
  const writer = new YasWriter()
    .u64(value.namespaceHandle)
    .bytes(value.operationId)
    .u16(value.durable ? g.YAS_KV_MUTATION_DURABLE : 0)
    .u16(value.mutations.length);
  for (const mutation of value.mutations)
    writer.bytesU32(encodeKvMutation(mutation));
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeKvBatch(bytes: Uint8Array): Required<YasKvBatch> {
  const cursor = new YasCursor(bytes);
  const namespaceHandle = cursor.u64("KV namespace handle");
  const operationId = new Uint8Array(cursor.take(16, "KV operation ID"));
  const flags = cursor.u16("KV BATCH flags");
  if (flags & ~g.YAS_KV_MUTATION_DURABLE)
    throw new YasProtocolError("unknown KV BATCH flags");
  const count = cursor.u16("KV batch item count");
  if (count === 0 || count > g.YAS_KV_MAX_BATCH_ITEMS)
    throw new YasProtocolError("invalid KV batch item count");
  const mutations: YasKvMutation[] = [];
  for (let index = 0; index < count; index++)
    mutations.push(decodeKvMutation(cursor.bytesU32("KV mutation")));
  const extensions = noRequiredExtensions(cursor, "KV BATCH extensions");
  cursor.end("KV BATCH");
  const value = {
    namespaceHandle,
    operationId,
    durable: flags !== 0,
    mutations,
    extensions,
  };
  encodeKvBatch(value);
  return value;
}

export function encodeKvMutationResult(value: YasKvMutationResult): Uint8Array {
  if (
    !Number.isInteger(value.status) ||
    value.status < 0 ||
    value.status > 0xffff
  )
    throw new YasProtocolError("invalid KV mutation status");
  if (value.status === g.YAS_STATUS_OK)
    validateRevision(value.modificationRevision, "KV modification revision");
  validateHash(value.contentHash);
  validateValueLength(value.byteLength);
  validateNoRequired(value.extensions, "KV mutation Result");
  return new YasWriter()
    .u16(value.status)
    .u16(0)
    .u64(value.modificationRevision)
    .i64(value.modifiedUnixNs)
    .bytes(value.contentHash)
    .u64(value.byteLength)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeKvMutationResult(bytes: Uint8Array): YasKvMutationResult {
  const cursor = new YasCursor(bytes);
  const status = cursor.u16("KV mutation status");
  if (cursor.u16("KV mutation Result reserved") !== 0)
    throw new YasProtocolError("KV mutation Result reserved field is nonzero");
  const modificationRevision = cursor.u64("KV modification revision");
  const modifiedUnixNs = cursor.i64("KV modified time");
  const contentHash = new Uint8Array(cursor.take(32, "KV content hash"));
  const byteLength = cursor.u64("KV value length");
  const extensions = noRequiredExtensions(
    cursor,
    "KV mutation Result extensions",
  );
  cursor.end("KV mutation Result");
  const value = {
    status,
    modificationRevision,
    modifiedUnixNs,
    contentHash,
    byteLength,
    extensions,
  };
  encodeKvMutationResult(value);
  return value;
}

export function encodeKvBatchResult(value: YasKvBatchResult): Uint8Array {
  validateRevision(value.storeRevision, "KV store revision");
  if (
    value.results.length === 0 ||
    value.results.length > g.YAS_KV_MAX_BATCH_ITEMS
  )
    throw new YasProtocolError("invalid KV batch Result count");
  validateNoRequired(value.extensions, "KV BATCH Result");
  const writer = new YasWriter()
    .u64(value.storeRevision)
    .u16(value.results.length)
    .u16(0);
  for (const result of value.results)
    writer.bytesU32(encodeKvMutationResult(result));
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeKvBatchResult(bytes: Uint8Array): YasKvBatchResult {
  const cursor = new YasCursor(bytes);
  const storeRevision = cursor.u64("KV store revision");
  const count = cursor.u16("KV batch Result count");
  if (cursor.u16("KV batch Result reserved") !== 0)
    throw new YasProtocolError("KV batch Result reserved field is nonzero");
  if (count === 0 || count > g.YAS_KV_MAX_BATCH_ITEMS)
    throw new YasProtocolError("invalid KV batch Result count");
  const results: YasKvMutationResult[] = [];
  for (let index = 0; index < count; index++)
    results.push(decodeKvMutationResult(cursor.bytesU32("KV mutation Result")));
  const extensions = noRequiredExtensions(cursor, "KV BATCH Result extensions");
  cursor.end("KV BATCH Result");
  const value = { storeRevision, results, extensions };
  encodeKvBatchResult(value);
  return value;
}

function encodePrecondition(writer: YasWriter, value: YasKvPrecondition): void {
  const kind =
    value.type === "any"
      ? g.YAS_KV_PRECONDITION_ANY
      : value.type === "absent"
        ? g.YAS_KV_PRECONDITION_ABSENT
        : value.type === "hash"
          ? g.YAS_KV_PRECONDITION_HASH
          : value.type === "revision"
            ? g.YAS_KV_PRECONDITION_REVISION
            : g.YAS_KV_PRECONDITION_HASH_AND_REVISION;
  writer.u8(kind).bytes(new Uint8Array(3));
  if (value.type === "hash") {
    validateHash(value.contentHash);
    writer.bytes(value.contentHash);
  } else if (value.type === "revision") {
    validateRevision(value.modificationRevision, "KV expected revision");
    writer.u64(value.modificationRevision);
  } else if (value.type === "hash-and-revision") {
    validateHash(value.contentHash);
    validateRevision(value.modificationRevision, "KV expected revision");
    writer.bytes(value.contentHash).u64(value.modificationRevision);
  }
}

function decodePrecondition(cursor: YasCursor): YasKvPrecondition {
  const kind = cursor.u8("KV precondition kind");
  reserved(cursor, 3, "KV precondition");
  if (kind === g.YAS_KV_PRECONDITION_ANY) return { type: "any" };
  if (kind === g.YAS_KV_PRECONDITION_ABSENT) return { type: "absent" };
  if (kind === g.YAS_KV_PRECONDITION_HASH)
    return {
      type: "hash",
      contentHash: new Uint8Array(cursor.take(32, "KV expected hash")),
    };
  if (kind === g.YAS_KV_PRECONDITION_REVISION) {
    const modificationRevision = cursor.u64("KV expected revision");
    validateRevision(modificationRevision, "KV expected revision");
    return { type: "revision", modificationRevision };
  }
  if (kind === g.YAS_KV_PRECONDITION_HASH_AND_REVISION) {
    const contentHash = new Uint8Array(cursor.take(32, "KV expected hash"));
    const modificationRevision = cursor.u64("KV expected revision");
    validateRevision(modificationRevision, "KV expected revision");
    return { type: "hash-and-revision", contentHash, modificationRevision };
  }
  throw new YasProtocolError("unknown KV precondition kind");
}

function encodeValueSource(writer: YasWriter, value: YasKvValueSource): void {
  if (value.type === "inline") {
    if (value.bytes.length > g.YAS_KV_MAX_INLINE_BYTES)
      throw new YasProtocolError("KV inline mutation value is too large");
    writer
      .u8(g.YAS_KV_VALUE_INLINE)
      .bytes(new Uint8Array(3))
      .bytesU32(value.bytes);
  } else {
    validateHandle(value.stagingHandle, "KV staging handle");
    writer
      .u8(g.YAS_KV_VALUE_STAGED)
      .bytes(new Uint8Array(3))
      .u64(value.stagingHandle);
  }
}

function decodeValueSource(cursor: YasCursor): YasKvValueSource {
  const kind = cursor.u8("KV value source kind");
  reserved(cursor, 3, "KV value source");
  if (kind === g.YAS_KV_VALUE_INLINE) {
    const bytes = new Uint8Array(cursor.bytesU32("KV inline mutation value"));
    if (bytes.length > g.YAS_KV_MAX_INLINE_BYTES)
      throw new YasProtocolError("KV inline mutation value is too large");
    return { type: "inline", bytes };
  }
  if (kind === g.YAS_KV_VALUE_STAGED) {
    const stagingHandle = cursor.u64("KV staging handle");
    validateHandle(stagingHandle, "KV staging handle");
    return { type: "staged", stagingHandle };
  }
  throw new YasProtocolError("unknown KV value source kind");
}

function validateMutationHeader(
  value: Pick<
    YasKvPut,
    "namespaceHandle" | "operationId" | "relativeKey" | "extensions"
  >,
): void {
  validateHandle(value.namespaceHandle, "KV namespace handle");
  validateOperationId(value.operationId);
  validateRelativeKey(value.relativeKey);
  validateNoRequired(value.extensions, "KV mutation");
}

function validateKvDelivery(
  value: YasInlineOrTransfer,
  direction: "download" | "upload",
): void {
  validateValueLength(value.byteLength);
  validateHash(value.contentHash);
  if (value.delivery === "inline") {
    if (
      value.bytes.length > g.YAS_KV_MAX_INLINE_BYTES ||
      BigInt(value.bytes.length) !== value.byteLength
    )
      throw new YasProtocolError("invalid KV inline GET value length");
  } else {
    validateKvDescriptor(value.descriptor, direction);
  }
}

function validateKvDescriptor(
  descriptor: YasTransferDescriptor,
  direction: "download" | "upload",
): void {
  const expectedDirection =
    direction === "download"
      ? g.YAS_TRANSFER_DIRECTION_SENDER_TO_RECEIVER
      : g.YAS_TRANSFER_DIRECTION_RECEIVER_TO_SENDER;
  if (
    descriptor.mode !== g.YAS_TRANSFER_MODE_BYTE ||
    descriptor.direction !== expectedDirection ||
    descriptor.contentFamily !== g.YAS_FAMILY_KV ||
    descriptor.contentKind !== g.YAS_KV_VALUE_CONTENT_KIND ||
    descriptor.contentVersion !== g.YAS_KV_VERSION ||
    descriptor.sensitiveContent !== true
  )
    throw new YasProtocolError("invalid KV value Transfer descriptor");
}

function validatePrefix(prefix: Uint8Array): void {
  if (prefix.length > g.YAS_KV_MAX_KEY_BYTES || prefix.includes(0))
    throw new YasProtocolError("invalid KV namespace prefix");
}

function validateRelativeKey(key: Uint8Array): void {
  if (key.length > g.YAS_KV_MAX_KEY_BYTES || key.includes(0))
    throw new YasProtocolError("invalid KV relative key");
}

export function validateKvFullKey(
  prefix: Uint8Array,
  relativeKey: Uint8Array,
): void {
  validatePrefix(prefix);
  validateRelativeKey(relativeKey);
  const length = prefix.length + relativeKey.length;
  if (length === 0 || length > g.YAS_KV_MAX_KEY_BYTES)
    throw new YasProtocolError("invalid KV full key length");
}

function validateOperationId(operationId: Uint8Array): void {
  if (operationId.length !== 16 || operationId.every((byte) => byte === 0))
    throw new YasProtocolError("invalid KV operation ID");
}

function validateHash(hash: Uint8Array): void {
  if (hash.length !== 32)
    throw new YasProtocolError("KV content hash must contain 32 bytes");
}

function validateHandle(handle: bigint, name: string): void {
  if (handle === 0n) throw new YasProtocolError(`${name} is zero`);
}

function validateRevision(revision: bigint, name: string): void {
  if (revision === 0n) throw new YasProtocolError(`${name} is zero`);
}

function validateValueLength(length: bigint): void {
  if (length < 0n || length > BigInt(g.YAS_KV_MAX_VALUE_BYTES))
    throw new YasProtocolError("KV value exceeds its byte limit");
}

function reserved(cursor: YasCursor, length: number, context: string): void {
  if (cursor.take(length, `${context} reserved`).some((byte) => byte !== 0))
    throw new YasProtocolError(`${context} reserved bytes are nonzero`);
}

function validateNoRequired(
  extensions: readonly YasExtension[] | undefined,
  context: string,
): void {
  if (extensions?.some((extension) => extension.required))
    throw new YasProtocolError(`${context} has an unknown required extension`);
}

function noRequiredExtensions(
  cursor: YasCursor,
  context: string,
): YasExtension[] {
  return decodeExtensions(cursor, new Set(), context);
}

export interface YasKvValue {
  bytes: Uint8Array;
  contentHash: Uint8Array;
  modificationRevision: bigint;
}

export interface YasKvMutationOptions {
  operationId?: Uint8Array;
  durable?: boolean;
  precondition?: YasKvPrecondition;
}

export type YasKvStateChange =
  | { type: "add" | "replace"; entry: YasKvEntry }
  | {
      type: "remove";
      relativeKey: Uint8Array;
      modificationRevision: bigint;
    };

export interface YasKvStateUpdate {
  phase: number;
  fromRevision: bigint;
  toRevision: bigint;
  changes: readonly YasKvStateChange[];
}

export class YasKvConflictError extends Error {
  constructor(readonly result: YasKvMutationResult) {
    super("KV mutation precondition failed");
    this.name = "YasKvConflictError";
  }
}

export type YasKvHashBytes = (
  bytes: Uint8Array,
) => Uint8Array | Promise<Uint8Array>;

export class YasKvClient {
  private readonly namespaces = new Set<YasKvNamespace>();
  private removeInvalidation: (() => void) | null;
  private generation = 0;
  private disposed = false;

  constructor(
    readonly connection: YasConnection,
    readonly hashBytes: YasKvHashBytes = defaultBlake3,
  ) {
    connection.family(g.YAS_FAMILY_KV, g.YAS_KV_VERSION);
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family !== undefined && family !== g.YAS_FAMILY_KV) return;
      this.generation++;
      for (const namespace of [...this.namespaces]) namespace.invalidate();
      this.namespaces.clear();
    });
  }

  async open(prefix: Uint8Array = new Uint8Array()): Promise<YasKvNamespace> {
    this.assertOpen();
    const generation = this.generation;
    const result = await this.connection.requestDecoded(
      g.YAS_FAMILY_KV,
      g.YAS_KV_OPEN,
      encodeKvOpen({ prefix }),
      decodeKvOpenResult,
    );
    const namespace = new YasKvNamespace(this, new Uint8Array(prefix), result);
    if (this.disposed || generation !== this.generation) {
      await namespace.close().catch(() => undefined);
      throw new YasProtocolError("KV client changed while OPEN was pending");
    }
    this.namespaces.add(namespace);
    return namespace;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.generation++;
    this.removeInvalidation?.();
    this.removeInvalidation = null;
    for (const namespace of [...this.namespaces])
      void namespace.close().catch(() => undefined);
    this.namespaces.clear();
  }

  release(namespace: YasKvNamespace): void {
    this.namespaces.delete(namespace);
  }

  private assertOpen(): void {
    if (this.disposed) throw new YasProtocolError("KV client is closed");
  }
}

export class YasKvNamespace {
  private readonly transfers;
  private readonly subscriptions = new Set<YasStateSubscription>();
  private readonly watchCancellations = new Set<(error: Error) => void>();
  private closed = false;
  private watchGeneration = 0;
  storeRevision: bigint;

  constructor(
    readonly client: YasKvClient,
    readonly prefix: Uint8Array,
    readonly opened: YasKvOpenResult,
  ) {
    this.storeRevision = opened.storeRevision;
    this.transfers = transfersFor(client.connection);
  }

  get handle(): bigint {
    return this.opened.namespaceHandle;
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.cancelPendingWatches("KV namespace closed while WATCH was pending");
    this.watchGeneration++;
    this.client.release(this);
    await Promise.allSettled(
      [...this.subscriptions].map((subscription) => subscription.unwatch()),
    );
    this.subscriptions.clear();
    await this.client.connection.request(
      g.YAS_FAMILY_KV,
      g.YAS_KV_CLOSE,
      encodeKvClose(this.handle),
    );
  }

  async get(
    relativeKey: Uint8Array,
    initialReceiveCredit = 1024n * 1024n,
  ): Promise<YasKvValue | null> {
    this.assertOpen();
    validateKvFullKey(this.prefix, relativeKey);
    const lease = this.transfers.reserveReceiveCredit(
      initialReceiveCredit,
      64n * 1024n,
    );
    let accepted = false;
    let released = false;
    try {
      const decoded = await this.client.connection.requestDecoded(
        g.YAS_FAMILY_KV,
        g.YAS_KV_GET,
        encodeKvGet({
          namespaceHandle: this.handle,
          relativeKey,
          initialReceiveCredit: lease.bytes,
        }),
        (body) => {
          const result = decodeKvGetResult(body);
          if (result.value.delivery === "inline") {
            lease.release();
            released = true;
            return { result, bytes: new Uint8Array(result.value.bytes) };
          }
          const transfer = this.transfers.acceptServerDescriptor(
            result.value.descriptor,
            lease,
          );
          accepted = true;
          return { result, transfer };
        },
      );
      const bytes =
        decoded.bytes !== undefined
          ? decoded.bytes
          : await decoded.transfer!.collect(decoded.result.value.byteLength);
      await verifyHash(
        this.client.hashBytes,
        bytes,
        decoded.result.value.contentHash,
      );
      return {
        bytes,
        contentHash: new Uint8Array(decoded.result.value.contentHash),
        modificationRevision: decoded.result.modificationRevision,
      };
    } catch (error) {
      if (!accepted && !released) lease.release();
      if (
        error instanceof YasResultError &&
        error.status === g.YAS_STATUS_NOT_FOUND
      )
        return null;
      throw error;
    }
  }

  async put(
    relativeKey: Uint8Array,
    bytes: Uint8Array,
    options: YasKvMutationOptions = {},
  ): Promise<YasKvMutationResult> {
    this.assertOpen();
    validateKvFullKey(this.prefix, relativeKey);
    if (bytes.length > g.YAS_KV_MAX_VALUE_BYTES)
      throw new YasProtocolError("KV value exceeds its byte limit");
    const source =
      bytes.length <= g.YAS_KV_MAX_INLINE_BYTES
        ? ({ type: "inline", bytes } as const)
        : await this.stage(bytes);
    const result = await this.client.connection.requestDecoded(
      g.YAS_FAMILY_KV,
      g.YAS_KV_PUT,
      encodeKvPut({
        namespaceHandle: this.handle,
        operationId: options.operationId ?? randomOperationId(),
        durable: options.durable,
        relativeKey,
        precondition: options.precondition ?? { type: "any" },
        value: source,
      }),
      decodeKvMutationResult,
    );
    return this.finishMutation(result);
  }

  async delete(
    relativeKey: Uint8Array,
    options: YasKvMutationOptions = {},
  ): Promise<YasKvMutationResult> {
    this.assertOpen();
    validateKvFullKey(this.prefix, relativeKey);
    const result = await this.client.connection.requestDecoded(
      g.YAS_FAMILY_KV,
      g.YAS_KV_DELETE,
      encodeKvDelete({
        namespaceHandle: this.handle,
        operationId: options.operationId ?? randomOperationId(),
        durable: options.durable,
        relativeKey,
        precondition: options.precondition ?? { type: "any" },
      }),
      decodeKvMutationResult,
    );
    return this.finishMutation(result);
  }

  async batch(
    mutations: readonly YasKvMutation[],
    options: Pick<YasKvMutationOptions, "operationId" | "durable"> = {},
  ): Promise<YasKvBatchResult> {
    this.assertOpen();
    for (const mutation of mutations)
      validateKvFullKey(this.prefix, mutation.relativeKey);
    const result = await this.client.connection.requestDecoded(
      g.YAS_FAMILY_KV,
      g.YAS_KV_BATCH,
      encodeKvBatch({
        namespaceHandle: this.handle,
        operationId: options.operationId ?? randomOperationId(),
        durable: options.durable,
        mutations,
      }),
      decodeKvBatchResult,
    );
    this.storeRevision = result.storeRevision;
    const conflict = result.results.find(
      (item) => item.status === g.YAS_STATUS_CONFLICT,
    );
    if (conflict) throw new YasKvConflictError(conflict);
    const failure = result.results.find(
      (item) => item.status !== g.YAS_STATUS_OK,
    );
    if (failure)
      throw new YasResultError(
        failure.status,
        new Uint8Array(),
        "KV batch item failed",
      );
    return result;
  }

  async watch(
    onUpdate: (update: YasKvStateUpdate) => void,
    options: YasWatchOptions & { inlineMax?: number } = {},
  ): Promise<YasStateSubscription> {
    this.assertOpen();
    const generation = this.watchGeneration;
    const inlineMax = options.inlineMax ?? g.YAS_KV_MAX_INLINE_BYTES;
    const operation = YasStateSubscription.watch(
      this.client.connection,
      g.YAS_FAMILY_KV,
      g.YAS_KV_WATCH,
      g.YAS_KV_UNWATCH,
      g.YAS_KV_STATE,
      g.YAS_KV_STATE_ACK,
      options,
      (batch) => {
        if (this.closed || generation !== this.watchGeneration) return;
        this.storeRevision = batch.toRevision;
        onUpdate(decodeStateBatch(batch));
      },
      {},
      (statePayload) => {
        if (
          !Number.isInteger(inlineMax) ||
          inlineMax < 0 ||
          inlineMax > g.YAS_KV_MAX_INLINE_BYTES
        )
          throw new YasProtocolError("invalid KV WATCH inline limit");
        return new YasWriter()
          .u64(this.handle)
          .u32(inlineMax)
          .u32(0)
          .bytesU32(statePayload)
          .finish();
      },
    ).then(async (subscription) => {
      if (this.closed || generation !== this.watchGeneration) {
        await subscription.unwatch().catch(() => undefined);
        throw new YasProtocolError(
          "KV namespace closed while WATCH was pending",
        );
      }
      this.subscriptions.add(subscription);
      return subscription;
    });
    let cancel!: (error: Error) => void;
    const cancelled = new Promise<never>((_resolve, reject) => {
      cancel = reject;
      this.watchCancellations.add(reject);
    });
    try {
      return await Promise.race([operation, cancelled]);
    } finally {
      this.watchCancellations.delete(cancel);
    }
  }

  invalidate(): void {
    if (this.closed) return;
    this.closed = true;
    this.cancelPendingWatches(
      "KV namespace invalidated while WATCH was pending",
    );
    this.watchGeneration++;
    this.client.release(this);
    for (const subscription of [...this.subscriptions])
      void subscription.unwatch().catch(() => undefined);
    this.subscriptions.clear();
  }

  private async stage(bytes: Uint8Array): Promise<YasKvValueSource> {
    const contentHash = await this.client.hashBytes(bytes);
    validateHash(contentHash);
    const result = await this.client.connection.requestDecoded(
      g.YAS_FAMILY_KV,
      g.YAS_KV_STAGE_VALUE,
      encodeKvStageValue({
        byteLength: BigInt(bytes.length),
        contentHash,
      }),
      decodeKvStageValueResult,
    );
    if (
      result.byteLength !== BigInt(bytes.length) ||
      !equalBytes(result.contentHash, contentHash)
    )
      throw new YasProtocolError("KV staging Result does not match request");
    const transfer = this.transfers.acceptServerUploadDescriptor(
      result.descriptor,
    );
    try {
      await transfer.write(bytes);
      transfer.closeWrite();
      await transfer.closed;
    } catch (error) {
      transfer.reset();
      throw error;
    }
    return { type: "staged", stagingHandle: result.stagingHandle };
  }

  private finishMutation(result: YasKvMutationResult): YasKvMutationResult {
    if (result.status === g.YAS_STATUS_CONFLICT)
      throw new YasKvConflictError(result);
    if (result.status !== g.YAS_STATUS_OK)
      throw new YasResultError(
        result.status,
        new Uint8Array(),
        "KV mutation failed",
      );
    this.storeRevision = result.modificationRevision;
    return result;
  }

  private assertOpen(): void {
    if (this.closed) throw new YasProtocolError("KV namespace is closed");
  }

  private cancelPendingWatches(message: string): void {
    const error = new YasProtocolError(message);
    for (const cancel of this.watchCancellations) cancel(error);
    this.watchCancellations.clear();
  }
}

function decodeStateBatch(batch: YasStateBatch): YasKvStateUpdate {
  const changes: YasKvStateChange[] = [];
  for (const record of batch.records) {
    if (record.kind === YAS_STATE_ADD || record.kind === YAS_STATE_REPLACE) {
      changes.push({
        type: record.kind === YAS_STATE_ADD ? "add" : "replace",
        entry: decodeKvEntry(record.body),
      });
    } else if (record.kind === YAS_STATE_REMOVE) {
      const removed = decodeKvRemovedEntry(record.body);
      changes.push({ type: "remove", ...removed });
    } else if (record.kind === YAS_STATE_PATCH) {
      throw new YasProtocolError("KV v1 does not define PATCH records");
    } else if (record.flags & 1) {
      throw new YasProtocolError("unknown required KV state record");
    }
  }
  return {
    phase: batch.phase,
    fromRevision: batch.fromRevision,
    toRevision: batch.toRevision,
    changes,
  };
}

function randomOperationId(): Uint8Array {
  const value = new Uint8Array(16);
  globalThis.crypto.getRandomValues(value);
  if (value.every((byte) => byte === 0)) value[0] = 1;
  return value;
}

async function verifyHash(
  hashBytes: YasKvHashBytes,
  bytes: Uint8Array,
  expected: Uint8Array,
): Promise<void> {
  const actual = await hashBytes(bytes);
  if (actual.length !== 32 || !equalBytes(actual, expected))
    throw new YasProtocolError("KV value failed BLAKE3 verification");
}

async function defaultBlake3(bytes: Uint8Array): Promise<Uint8Array> {
  const { blake3_hash } = await import("@yas-run/browser");
  return blake3_hash(bytes);
}
