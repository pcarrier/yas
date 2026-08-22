/** YAS extension-supervisor family v1 codecs and browser client. */

import * as g from "./generated";
import type { YasConnection } from "./session";
import {
  YAS_STATE_ADD,
  YAS_STATE_DELTA,
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
  YasResultError,
  YasWriter,
  decodeExtensions,
  encodeExtensions,
  type YasExtension,
  type YasTypedRecord,
} from "./wire";

export {
  YAS_EXTENSION_ATTEMPT_CONTEXT,
  YAS_EXTENSION_CONTROL,
  YAS_EXTENSION_DEPLOY,
  YAS_EXTENSION_DISCOVER_COMMANDS,
  YAS_EXTENSION_FOLLOW,
  YAS_EXTENSION_OBJECT_BEGIN,
  YAS_EXTENSION_OBJECT_COMMIT,
  YAS_EXTENSION_STATE,
  YAS_EXTENSION_STATE_ACK,
  YAS_EXTENSION_UNWATCH,
  YAS_EXTENSION_VERSION,
  YAS_EXTENSION_WATCH,
  YAS_FAMILY_EXTENSION,
} from "./generated";

export interface YasExtensionRuntimeLimits {
  memoryBytes: bigint;
  stackBytes: bigint;
  maxActiveJobs: number;
  maxPendingJobs: number;
  maxJobBytes: bigint;
  slowConsumerTimeoutNs: bigint;
  extensions?: readonly YasExtension[];
}

export interface YasExtensionObjectBegin {
  operationId: Uint8Array;
  contentHash: Uint8Array;
  byteLength: bigint;
  extensions?: readonly YasExtension[];
}

export interface YasExtensionObjectBeginResult {
  disposition: number;
  stagingHandle: bigint;
  descriptor?: YasTransferDescriptor;
  extensions: readonly YasExtension[];
}

export interface YasExtensionObjectCommit {
  stagingHandle: bigint;
  operationId: Uint8Array;
  contentHash: Uint8Array;
  byteLength: bigint;
  extensions?: readonly YasExtension[];
}

export interface YasExtensionDeploy {
  operationId: Uint8Array;
  expectedExtensionHandle: bigint;
  expectedGeneration: bigint;
  expectedDefinitionRevision: bigint;
  flags: number;
  runtime: number;
  restartPolicy: number;
  name: string;
  contentHash: Uint8Array;
  argv: readonly Uint8Array[];
  runtimeLimits: YasExtensionRuntimeLimits;
  extensions?: readonly YasExtension[];
}

export interface YasExtensionDefinitionIdentity {
  extensionHandle: bigint;
  generation: bigint;
  definitionRevision: bigint;
  extensions: readonly YasExtension[];
}

export interface YasExtensionControl {
  extensionHandle: bigint;
  generation: bigint;
  expectedDefinitionRevision: bigint;
  operationId: Uint8Array;
  action: number;
  extensions?: readonly YasExtension[];
}

export interface YasExtensionFollowRequest {
  extensionHandle: bigint;
  generation: bigint;
  attempt: bigint;
  fromSequence: bigint;
  initialReceiveCredit: bigint;
  extensions?: readonly YasExtension[];
}

export interface YasExtensionFollowResult {
  attempt: bigint;
  firstSequence: bigint;
  throughSequence: bigint;
  descriptor: YasTransferDescriptor;
  extensions: readonly YasExtension[];
}

export interface YasExtensionExitRecord {
  kind: number;
  code: number;
  attempt: bigint;
  serverNs: bigint;
  detail: string;
  extensions: readonly YasExtension[];
}

export interface YasExtensionRecord {
  extensionHandle: bigint;
  generation: bigint;
  definitionRevision: bigint;
  phase: number;
  runtime: number;
  restartPolicy: number;
  flags: number;
  attempt: bigint;
  lastRunningAttempt: bigint;
  taskId: number;
  nextStartUnixMs: bigint;
  directoryRevision: bigint;
  contentHash: Uint8Array;
  name: string;
  lastExit?: YasExtensionExitRecord;
  runtimeLimits: YasExtensionRuntimeLimits;
  extensions: readonly YasExtension[];
}

export interface YasExtensionRemovedRecord {
  extensionHandle: bigint;
  generation: bigint;
}

export interface YasExtensionAttemptContext {
  extensionHandle: bigint;
  generation: bigint;
  definitionRevision: bigint;
  attempt: bigint;
  taskId: number;
  flags: number;
  runtime: number;
  contentHash: Uint8Array;
  name: string;
  argv: readonly Uint8Array[];
  extensions: readonly YasExtension[];
}

export interface YasExtensionOutputRecord {
  kind: number;
  sequence: bigint;
  serverNs: bigint;
  data: Uint8Array;
}

export interface YasExtensionOutputBatch {
  firstSequence: bigint;
  records: readonly YasExtensionOutputRecord[];
}

export interface YasExtensionDiscoverCommands {
  directoryRevision: bigint;
  cursor: bigint;
  maxRecords: number;
  extensions?: readonly YasExtension[];
}

export interface YasExtensionCommandRecord {
  extensionHandle: bigint;
  generation: bigint;
  definitionRevision: bigint;
  contentHash: Uint8Array;
  listenerHandle: bigint;
  listenerGeneration: bigint;
  name: string;
  listenerName: string;
  descriptor: string;
  extensions: readonly YasExtension[];
}

export interface YasExtensionCommandPage {
  directoryRevision: bigint;
  nextCursor: bigint;
  records: readonly YasExtensionCommandRecord[];
}

export interface YasExtensionSnapshot {
  revision: bigint;
  definitions: readonly YasExtensionRecord[];
}

export interface YasExtensionLimits {
  maxNameBytes: number;
  maxArgs: number;
  maxArgumentBytes: bigint;
  maxObjectBytes: bigint;
  maxOutputRecordBytes: number;
  maxCommandDescriptorBytes: number;
  maxCommandRecords: number;
  maxDefinitions: number;
  maxObjectStagesPerSession: number;
  maxFollowsPerSession: number;
  maxRunningAttempts: number;
  maxMemoryBytes: bigint;
  maxJobBytes: bigint;
  /** Durable newest-N persistent-mutation replay horizon. */
  maxMutationReplays: number;
}

export interface YasExtensionObjectUpload {
  stagingHandle: bigint;
  transfer: YasTransfer;
  extensions: readonly YasExtension[];
}

interface YasExtensionObjectBeginOperation {
  payloadKey: string;
  pending: Promise<YasExtensionObjectUpload | null> | null;
  result: YasExtensionObjectUpload | null;
  hasResult: boolean;
  retainPayload: boolean;
}

interface YasExtensionObjectStage {
  operationKey: string;
  upload: YasExtensionObjectUpload;
  expiresServerNs: bigint;
  removeTerminalListener: () => void;
  removeResetListener: () => void;
}

export interface YasExtensionFollowStream {
  attempt: bigint;
  firstSequence: bigint;
  throughSequence: bigint;
  transfer: YasTransfer;
  read(): Promise<YasExtensionOutputBatch | null>;
  reset(): void;
}

export function encodeExtensionRuntimeLimits(
  value: YasExtensionRuntimeLimits,
): Uint8Array {
  validateRuntimeLimits(value);
  return new YasWriter()
    .u64(value.memoryBytes)
    .u64(value.stackBytes)
    .u32(value.maxActiveJobs)
    .u32(value.maxPendingJobs)
    .u64(value.maxJobBytes)
    .u64(value.slowConsumerTimeoutNs)
    .bytes(encodeKnownExtensions(value.extensions, "Extension runtime limits"))
    .finish();
}

export function decodeExtensionRuntimeLimits(
  bytes: Uint8Array,
): YasExtensionRuntimeLimits {
  const cursor = new YasCursor(bytes);
  const value = {
    memoryBytes: cursor.u64("Extension memory limit"),
    stackBytes: cursor.u64("Extension stack limit"),
    maxActiveJobs: cursor.u32("Extension active-job limit"),
    maxPendingJobs: cursor.u32("Extension pending-job limit"),
    maxJobBytes: cursor.u64("Extension job byte limit"),
    slowConsumerTimeoutNs: cursor.u64("Extension slow-consumer timeout"),
    extensions: decodeExtensions(cursor, new Set(), "Extension runtime limits"),
  };
  cursor.end("Extension runtime limits");
  validateRuntimeLimits(value);
  return value;
}

export function encodeExtensionObjectBegin(
  value: YasExtensionObjectBegin,
): Uint8Array {
  requireOperationId(value.operationId, "Extension OBJECT_BEGIN");
  requireHash(value.contentHash, "Extension object");
  requireObjectLength(value.byteLength);
  return new YasWriter()
    .bytes(value.operationId)
    .bytes(value.contentHash)
    .u64(value.byteLength)
    .bytes(encodeKnownExtensions(value.extensions, "Extension OBJECT_BEGIN"))
    .finish();
}

export function decodeExtensionObjectBegin(
  bytes: Uint8Array,
): YasExtensionObjectBegin {
  const cursor = new YasCursor(bytes);
  const value = {
    operationId: new Uint8Array(cursor.take(16, "Extension operation ID")),
    contentHash: new Uint8Array(cursor.take(32, "Extension object hash")),
    byteLength: cursor.u64("Extension object length"),
    extensions: decodeExtensions(cursor, new Set(), "Extension OBJECT_BEGIN"),
  };
  cursor.end("Extension OBJECT_BEGIN");
  encodeExtensionObjectBegin(value);
  return value;
}

export function encodeExtensionObjectBeginResult(
  value: YasExtensionObjectBeginResult,
): Uint8Array {
  validateObjectBeginResult(value);
  return new YasWriter()
    .u8(value.disposition)
    .bytes(new Uint8Array(7))
    .u64(value.stagingHandle)
    .bytesU32(
      value.descriptor
        ? encodeTransferDescriptor(value.descriptor)
        : new Uint8Array(),
    )
    .bytes(
      encodeKnownExtensions(value.extensions, "Extension OBJECT_BEGIN Result"),
    )
    .finish();
}

export function decodeExtensionObjectBeginResult(
  bytes: Uint8Array,
): YasExtensionObjectBeginResult {
  const cursor = new YasCursor(bytes);
  const disposition = cursor.u8("Extension object disposition");
  requireZero(cursor.take(7, "Extension object reserved"), "Extension object");
  const stagingHandle = cursor.u64("Extension staging handle");
  const descriptorBytes = cursor.bytesU32("Extension object descriptor");
  const value = {
    disposition,
    stagingHandle,
    descriptor: descriptorBytes.length
      ? decodeDescriptor(descriptorBytes)
      : undefined,
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Extension OBJECT_BEGIN Result",
    ),
  };
  cursor.end("Extension OBJECT_BEGIN Result");
  validateObjectBeginResult(value);
  return value;
}

export function encodeExtensionObjectCommit(
  value: YasExtensionObjectCommit,
): Uint8Array {
  requireHandle(value.stagingHandle, "Extension staging handle");
  requireOperationId(value.operationId, "Extension OBJECT_COMMIT");
  requireHash(value.contentHash, "Extension object");
  requireObjectLength(value.byteLength);
  return new YasWriter()
    .u64(value.stagingHandle)
    .bytes(value.operationId)
    .bytes(value.contentHash)
    .u64(value.byteLength)
    .bytes(encodeKnownExtensions(value.extensions, "Extension OBJECT_COMMIT"))
    .finish();
}

export function decodeExtensionObjectCommit(
  bytes: Uint8Array,
): YasExtensionObjectCommit {
  const cursor = new YasCursor(bytes);
  const value = {
    stagingHandle: cursor.u64("Extension staging handle"),
    operationId: new Uint8Array(cursor.take(16, "Extension operation ID")),
    contentHash: new Uint8Array(cursor.take(32, "Extension object hash")),
    byteLength: cursor.u64("Extension object length"),
    extensions: decodeExtensions(cursor, new Set(), "Extension OBJECT_COMMIT"),
  };
  cursor.end("Extension OBJECT_COMMIT");
  encodeExtensionObjectCommit(value);
  return value;
}

export function encodeExtensionDeploy(value: YasExtensionDeploy): Uint8Array {
  validateDeploy(value);
  const writer = new YasWriter()
    .bytes(value.operationId)
    .u64(value.expectedExtensionHandle)
    .u64(value.expectedGeneration)
    .u64(value.expectedDefinitionRevision)
    .u16(value.flags)
    .u8(value.runtime)
    .u8(value.restartPolicy)
    .utf8U16(value.name)
    .bytes(value.contentHash)
    .u16(value.argv.length);
  for (const argument of value.argv) writer.bytesU32(argument);
  return writer
    .bytesU32(encodeExtensionRuntimeLimits(value.runtimeLimits))
    .bytes(encodeDeployExtensions(value.extensions))
    .finish();
}

export function decodeExtensionDeploy(bytes: Uint8Array): YasExtensionDeploy {
  const cursor = new YasCursor(bytes);
  const operationId = new Uint8Array(cursor.take(16, "Extension operation ID"));
  const expectedExtensionHandle = cursor.u64("Extension expected handle");
  const expectedGeneration = cursor.u64("Extension expected generation");
  const expectedDefinitionRevision = cursor.u64(
    "Extension expected definition revision",
  );
  const flags = cursor.u16("Extension definition flags");
  const runtime = cursor.u8("Extension runtime");
  const restartPolicy = cursor.u8("Extension restart policy");
  const name = cursor.utf8U16("Extension name");
  const contentHash = new Uint8Array(cursor.take(32, "Extension object hash"));
  const count = cursor.u16("Extension argument count");
  if (
    count > g.YAS_EXTENSION_MAX_ARGS ||
    count > Math.floor(cursor.remaining / 4)
  )
    throw new YasProtocolError("invalid Extension argument count");
  const argv: Uint8Array[] = [];
  for (let index = 0; index < count; index++)
    argv.push(new Uint8Array(cursor.bytesU32("Extension argument")));
  const value = {
    operationId,
    expectedExtensionHandle,
    expectedGeneration,
    expectedDefinitionRevision,
    flags,
    runtime,
    restartPolicy,
    name,
    contentHash,
    argv,
    runtimeLimits: decodeExtensionRuntimeLimits(
      cursor.bytesU32("Extension runtime limits"),
    ),
    extensions: decodeExtensions(
      cursor,
      new Set([g.YAS_EXTENSION_DEPLOY_PRESERVE_ARGV_TAG]),
      "Extension DEPLOY",
    ),
  };
  cursor.end("Extension DEPLOY");
  validateDeploy(value);
  return value;
}

export function encodeExtensionDefinitionIdentity(
  value: YasExtensionDefinitionIdentity,
): Uint8Array {
  validateIdentity(value.extensionHandle, value.generation);
  requireRevision(value.definitionRevision, "Extension definition revision");
  return new YasWriter()
    .u64(value.extensionHandle)
    .u64(value.generation)
    .u64(value.definitionRevision)
    .bytes(
      encodeKnownExtensions(value.extensions, "Extension definition identity"),
    )
    .finish();
}

export function decodeExtensionDefinitionIdentity(
  bytes: Uint8Array,
): YasExtensionDefinitionIdentity {
  const cursor = new YasCursor(bytes);
  const value = {
    extensionHandle: cursor.u64("Extension handle"),
    generation: cursor.u64("Extension generation"),
    definitionRevision: cursor.u64("Extension definition revision"),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Extension definition identity",
    ),
  };
  cursor.end("Extension definition identity");
  encodeExtensionDefinitionIdentity(value);
  return value;
}

export function encodeExtensionControl(value: YasExtensionControl): Uint8Array {
  validateIdentity(value.extensionHandle, value.generation);
  requireOperationId(value.operationId, "Extension CONTROL");
  if (
    value.action < g.YAS_EXTENSION_CONTROL_STOP ||
    value.action > g.YAS_EXTENSION_CONTROL_REMOVE
  )
    throw new YasProtocolError("invalid Extension control action");
  return new YasWriter()
    .u64(value.extensionHandle)
    .u64(value.generation)
    .u64(value.expectedDefinitionRevision)
    .bytes(value.operationId)
    .u8(value.action)
    .bytes(new Uint8Array(7))
    .bytes(encodeKnownExtensions(value.extensions, "Extension CONTROL"))
    .finish();
}

export function decodeExtensionControl(bytes: Uint8Array): YasExtensionControl {
  const cursor = new YasCursor(bytes);
  const extensionHandle = cursor.u64("Extension handle");
  const generation = cursor.u64("Extension generation");
  const expectedDefinitionRevision = cursor.u64(
    "Extension expected definition revision",
  );
  const operationId = new Uint8Array(cursor.take(16, "Extension operation ID"));
  const action = cursor.u8("Extension control action");
  requireZero(
    cursor.take(7, "Extension CONTROL reserved"),
    "Extension CONTROL",
  );
  const value = {
    extensionHandle,
    generation,
    expectedDefinitionRevision,
    operationId,
    action,
    extensions: decodeExtensions(cursor, new Set(), "Extension CONTROL"),
  };
  cursor.end("Extension CONTROL");
  encodeExtensionControl(value);
  return value;
}

export function encodeExtensionFollow(
  value: YasExtensionFollowRequest,
): Uint8Array {
  validateIdentity(value.extensionHandle, value.generation);
  if (value.initialReceiveCredit === 0n)
    throw new YasProtocolError("zero Extension follow receive credit");
  return new YasWriter()
    .u64(value.extensionHandle)
    .u64(value.generation)
    .u64(value.attempt)
    .u64(value.fromSequence)
    .u64(value.initialReceiveCredit)
    .bytes(encodeKnownExtensions(value.extensions, "Extension FOLLOW"))
    .finish();
}

export function decodeExtensionFollow(
  bytes: Uint8Array,
): YasExtensionFollowRequest {
  const cursor = new YasCursor(bytes);
  const value = {
    extensionHandle: cursor.u64("Extension handle"),
    generation: cursor.u64("Extension generation"),
    attempt: cursor.u64("Extension attempt"),
    fromSequence: cursor.u64("Extension first requested sequence"),
    initialReceiveCredit: cursor.u64("Extension follow receive credit"),
    extensions: decodeExtensions(cursor, new Set(), "Extension FOLLOW"),
  };
  cursor.end("Extension FOLLOW");
  encodeExtensionFollow(value);
  return value;
}

export function encodeExtensionFollowResult(
  value: YasExtensionFollowResult,
): Uint8Array {
  validateFollowResult(value);
  return new YasWriter()
    .u64(value.attempt)
    .u64(value.firstSequence)
    .u64(value.throughSequence)
    .bytesU32(encodeTransferDescriptor(value.descriptor))
    .bytes(encodeKnownExtensions(value.extensions, "Extension FOLLOW Result"))
    .finish();
}

export function decodeExtensionFollowResult(
  bytes: Uint8Array,
): YasExtensionFollowResult {
  const cursor = new YasCursor(bytes);
  const value = {
    attempt: cursor.u64("Extension attempt"),
    firstSequence: cursor.u64("Extension first sequence"),
    throughSequence: cursor.u64("Extension through sequence"),
    descriptor: decodeDescriptor(
      cursor.bytesU32("Extension follow descriptor"),
    ),
    extensions: decodeExtensions(cursor, new Set(), "Extension FOLLOW Result"),
  };
  cursor.end("Extension FOLLOW Result");
  validateFollowResult(value);
  return value;
}

export function encodeExtensionExitRecord(
  value: YasExtensionExitRecord,
): Uint8Array {
  validateExit(value);
  return new YasWriter()
    .u8(value.kind)
    .bytes(new Uint8Array(3))
    .i32(value.code)
    .u64(value.attempt)
    .u64(value.serverNs)
    .utf8U32(value.detail)
    .bytes(encodeKnownExtensions(value.extensions, "Extension exit record"))
    .finish();
}

export function decodeExtensionExitRecord(
  bytes: Uint8Array,
): YasExtensionExitRecord {
  const cursor = new YasCursor(bytes);
  const value = decodeExit(cursor);
  cursor.end("Extension exit record");
  return value;
}

export function encodeExtensionRecord(value: YasExtensionRecord): Uint8Array {
  validateExtensionRecord(value);
  return new YasWriter()
    .u64(value.extensionHandle)
    .u64(value.generation)
    .u64(value.definitionRevision)
    .u8(value.phase)
    .u8(value.runtime)
    .u8(value.restartPolicy)
    .u8(0)
    .u16(value.flags)
    .u16(0)
    .u64(value.attempt)
    .u64(value.lastRunningAttempt)
    .u32(value.taskId)
    .u32(0)
    .u64(value.nextStartUnixMs)
    .u64(value.directoryRevision)
    .bytes(value.contentHash)
    .utf8U16(value.name)
    .bytesU32(
      value.lastExit
        ? encodeExtensionExitRecord(value.lastExit)
        : new Uint8Array(),
    )
    .bytesU32(encodeExtensionRuntimeLimits(value.runtimeLimits))
    .bytes(encodeKnownExtensions(value.extensions, "Extension state record"))
    .finish();
}

export function decodeExtensionRecord(bytes: Uint8Array): YasExtensionRecord {
  const cursor = new YasCursor(bytes);
  const extensionHandle = cursor.u64("Extension handle");
  const generation = cursor.u64("Extension generation");
  const definitionRevision = cursor.u64("Extension definition revision");
  const phase = cursor.u8("Extension phase");
  const runtime = cursor.u8("Extension runtime");
  const restartPolicy = cursor.u8("Extension restart policy");
  if (cursor.u8("Extension state reserved") !== 0)
    throw new YasProtocolError("Extension state reserved byte is nonzero");
  const flags = cursor.u16("Extension definition flags");
  if (cursor.u16("Extension state reserved") !== 0)
    throw new YasProtocolError("Extension state reserved field is nonzero");
  const attempt = cursor.u64("Extension attempt");
  const lastRunningAttempt = cursor.u64("Extension last running attempt");
  const taskId = cursor.u32("Extension task ID");
  if (cursor.u32("Extension state reserved") !== 0)
    throw new YasProtocolError("Extension state reserved field is nonzero");
  const nextStartUnixMs = cursor.u64("Extension next-start Unix time");
  const directoryRevision = cursor.u64("Extension directory revision");
  const contentHash = new Uint8Array(cursor.take(32, "Extension object hash"));
  const name = cursor.utf8U16("Extension name");
  const exitBytes = cursor.bytesU32("Extension last exit");
  const value = {
    extensionHandle,
    generation,
    definitionRevision,
    phase,
    runtime,
    restartPolicy,
    flags,
    attempt,
    lastRunningAttempt,
    taskId,
    nextStartUnixMs,
    directoryRevision,
    contentHash,
    name,
    lastExit: exitBytes.length
      ? decodeExtensionExitRecord(exitBytes)
      : undefined,
    runtimeLimits: decodeExtensionRuntimeLimits(
      cursor.bytesU32("Extension runtime limits"),
    ),
    extensions: decodeExtensions(cursor, new Set(), "Extension state record"),
  };
  cursor.end("Extension state record");
  validateExtensionRecord(value);
  return value;
}

export function encodeExtensionRemovedRecord(
  value: YasExtensionRemovedRecord,
): Uint8Array {
  validateIdentity(value.extensionHandle, value.generation);
  return new YasWriter()
    .u64(value.extensionHandle)
    .u64(value.generation)
    .finish();
}

export function decodeExtensionRemovedRecord(
  bytes: Uint8Array,
): YasExtensionRemovedRecord {
  const cursor = new YasCursor(bytes);
  const value = {
    extensionHandle: cursor.u64("removed Extension handle"),
    generation: cursor.u64("removed Extension generation"),
  };
  cursor.end("Extension remove record");
  validateIdentity(value.extensionHandle, value.generation);
  return value;
}

export function encodeExtensionAttemptContext(
  value: YasExtensionAttemptContext,
): Uint8Array {
  validateAttemptContext(value);
  const writer = new YasWriter()
    .u64(value.extensionHandle)
    .u64(value.generation)
    .u64(value.definitionRevision)
    .u64(value.attempt)
    .u32(value.taskId)
    .u16(value.flags)
    .u8(value.runtime)
    .u8(0)
    .bytes(value.contentHash)
    .utf8U16(value.name)
    .u16(value.argv.length);
  for (const argument of value.argv) writer.bytesU32(argument);
  return writer
    .bytes(encodeKnownExtensions(value.extensions, "Extension attempt context"))
    .finish();
}

export function decodeExtensionAttemptContext(
  bytes: Uint8Array,
): YasExtensionAttemptContext {
  const cursor = new YasCursor(bytes);
  const extensionHandle = cursor.u64("Extension handle");
  const generation = cursor.u64("Extension generation");
  const definitionRevision = cursor.u64("Extension definition revision");
  const attempt = cursor.u64("Extension attempt");
  const taskId = cursor.u32("Extension task ID");
  const flags = cursor.u16("Extension definition flags");
  const runtime = cursor.u8("Extension runtime");
  if (cursor.u8("Extension attempt reserved") !== 0)
    throw new YasProtocolError("Extension attempt reserved byte is nonzero");
  const contentHash = new Uint8Array(cursor.take(32, "Extension object hash"));
  const name = cursor.utf8U16("Extension name");
  const count = cursor.u16("Extension argument count");
  if (
    count > g.YAS_EXTENSION_MAX_ARGS ||
    count > Math.floor(cursor.remaining / 4)
  )
    throw new YasProtocolError("invalid Extension argument count");
  const argv: Uint8Array[] = [];
  for (let index = 0; index < count; index++)
    argv.push(new Uint8Array(cursor.bytesU32("Extension argument")));
  const value = {
    extensionHandle,
    generation,
    definitionRevision,
    attempt,
    taskId,
    flags,
    runtime,
    contentHash,
    name,
    argv,
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Extension attempt context",
    ),
  };
  cursor.end("Extension attempt context");
  validateAttemptContext(value);
  return value;
}

export function encodeExtensionOutputBatch(
  value: YasExtensionOutputBatch,
): Uint8Array {
  if (value.records.length === 0 || value.records.length > 0xffff)
    throw new YasProtocolError("invalid Extension output record count");
  const writer = new YasWriter()
    .u64(value.firstSequence)
    .u16(value.records.length)
    .u16(0);
  value.records.forEach((record, index) => {
    validateOutputRecord(record);
    if (record.sequence !== value.firstSequence + BigInt(index))
      throw new YasProtocolError("non-consecutive Extension output records");
    writer
      .u8(record.kind)
      .bytes(new Uint8Array(3))
      .u64(record.sequence)
      .u64(record.serverNs)
      .bytesU32(record.data);
  });
  const output = writer.finish();
  if (output.length > g.YAS_EXTENSION_MAX_OUTPUT_BATCH_BYTES)
    throw new YasProtocolError("Extension output batch exceeds its byte limit");
  return output;
}

export function decodeExtensionOutputBatch(
  bytes: Uint8Array,
): YasExtensionOutputBatch {
  if (bytes.length > g.YAS_EXTENSION_MAX_OUTPUT_BATCH_BYTES)
    throw new YasProtocolError("Extension output batch exceeds its byte limit");
  const cursor = new YasCursor(bytes);
  const firstSequence = cursor.u64("Extension first output sequence");
  const count = cursor.u16("Extension output record count");
  if (
    cursor.u16("Extension output reserved") !== 0 ||
    count === 0 ||
    count > Math.floor(cursor.remaining / 24)
  )
    throw new YasProtocolError(
      "invalid Extension output count or reserved field",
    );
  const records: YasExtensionOutputRecord[] = [];
  for (let index = 0; index < count; index++) {
    const kind = cursor.u8("Extension output kind");
    requireZero(
      cursor.take(3, "Extension output reserved"),
      "Extension output",
    );
    const record = {
      kind,
      sequence: cursor.u64("Extension output sequence"),
      serverNs: cursor.u64("Extension output time"),
      data: new Uint8Array(cursor.bytesU32("Extension output data")),
    };
    validateOutputRecord(record);
    if (record.sequence !== firstSequence + BigInt(index))
      throw new YasProtocolError("non-consecutive Extension output records");
    records.push(record);
  }
  cursor.end("Extension output batch");
  return { firstSequence, records };
}

export function encodeExtensionDiscoverCommands(
  value: YasExtensionDiscoverCommands,
): Uint8Array {
  if (
    !Number.isInteger(value.maxRecords) ||
    value.maxRecords < 0 ||
    value.maxRecords > g.YAS_EXTENSION_MAX_COMMAND_RECORDS
  )
    throw new YasProtocolError("invalid Extension command page size");
  return new YasWriter()
    .u64(value.directoryRevision)
    .u64(value.cursor)
    .u16(value.maxRecords)
    .u16(0)
    .bytes(
      encodeKnownExtensions(value.extensions, "Extension DISCOVER_COMMANDS"),
    )
    .finish();
}

export function decodeExtensionDiscoverCommands(
  bytes: Uint8Array,
): YasExtensionDiscoverCommands {
  const cursor = new YasCursor(bytes);
  const directoryRevision = cursor.u64("Extension directory revision");
  const pageCursor = cursor.u64("Extension command cursor");
  const maxRecords = cursor.u16("Extension command page size");
  if (cursor.u16("Extension discovery reserved") !== 0)
    throw new YasProtocolError("Extension discovery reserved field is nonzero");
  const value = {
    directoryRevision,
    cursor: pageCursor,
    maxRecords,
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Extension DISCOVER_COMMANDS",
    ),
  };
  cursor.end("Extension DISCOVER_COMMANDS");
  encodeExtensionDiscoverCommands(value);
  return value;
}

export function encodeExtensionCommandRecord(
  value: YasExtensionCommandRecord,
): Uint8Array {
  validateCommandRecord(value);
  return new YasWriter()
    .u64(value.extensionHandle)
    .u64(value.generation)
    .u64(value.definitionRevision)
    .bytes(value.contentHash)
    .u64(value.listenerHandle)
    .u64(value.listenerGeneration)
    .utf8U16(value.name)
    .utf8U16(value.listenerName)
    .utf8U32(value.descriptor)
    .bytes(encodeKnownExtensions(value.extensions, "Extension command record"))
    .finish();
}

export function decodeExtensionCommandRecord(
  bytes: Uint8Array,
): YasExtensionCommandRecord {
  const cursor = new YasCursor(bytes);
  const value = decodeCommandRecord(cursor);
  cursor.end("Extension command record");
  return value;
}

export function encodeExtensionCommandPage(
  value: YasExtensionCommandPage,
): Uint8Array {
  requireRevision(value.directoryRevision, "Extension directory revision");
  if (value.records.length > g.YAS_EXTENSION_MAX_COMMAND_RECORDS)
    throw new YasProtocolError("too many Extension command records");
  const writer = new YasWriter()
    .u64(value.directoryRevision)
    .u64(value.nextCursor)
    .u16(value.records.length)
    .u16(0);
  for (const record of value.records)
    writer.bytes(encodeExtensionCommandRecord(record));
  const output = writer.finish();
  if (output.length > g.YAS_EXTENSION_MAX_COMMAND_PAGE_BYTES)
    throw new YasProtocolError("Extension command page exceeds its byte limit");
  return output;
}

export function decodeExtensionCommandPage(
  bytes: Uint8Array,
): YasExtensionCommandPage {
  if (bytes.length > g.YAS_EXTENSION_MAX_COMMAND_PAGE_BYTES)
    throw new YasProtocolError("Extension command page exceeds its byte limit");
  const cursor = new YasCursor(bytes);
  const directoryRevision = cursor.u64("Extension directory revision");
  const nextCursor = cursor.u64("Extension next command cursor");
  const count = cursor.u16("Extension command record count");
  if (
    cursor.u16("Extension command reserved") !== 0 ||
    count > g.YAS_EXTENSION_MAX_COMMAND_RECORDS ||
    count > Math.floor(cursor.remaining / 84)
  )
    throw new YasProtocolError(
      "invalid Extension command count or reserved field",
    );
  const records: YasExtensionCommandRecord[] = [];
  for (let index = 0; index < count; index++)
    records.push(decodeCommandRecord(cursor));
  cursor.end("Extension command page");
  const value = { directoryRevision, nextCursor, records };
  encodeExtensionCommandPage(value);
  return value;
}

export function extensionLimitsFromExtensions(
  extensions: readonly YasExtension[],
): YasExtensionLimits {
  const value = {
    maxNameBytes: limit32(extensions, g.YAS_EXTENSION_LIMIT_MAX_NAME_BYTES),
    maxArgs: limit32(extensions, g.YAS_EXTENSION_LIMIT_MAX_ARGS),
    maxArgumentBytes: limit64(
      extensions,
      g.YAS_EXTENSION_LIMIT_MAX_ARGUMENT_BYTES,
    ),
    maxObjectBytes: limit64(extensions, g.YAS_EXTENSION_LIMIT_MAX_OBJECT_BYTES),
    maxOutputRecordBytes: limit32(
      extensions,
      g.YAS_EXTENSION_LIMIT_MAX_OUTPUT_RECORD_BYTES,
    ),
    maxCommandDescriptorBytes: limit32(
      extensions,
      g.YAS_EXTENSION_LIMIT_MAX_COMMAND_DESCRIPTOR_BYTES,
    ),
    maxCommandRecords: limit32(
      extensions,
      g.YAS_EXTENSION_LIMIT_MAX_COMMAND_RECORDS,
    ),
    maxDefinitions: limit32(extensions, g.YAS_EXTENSION_LIMIT_MAX_DEFINITIONS),
    maxObjectStagesPerSession: limit32(
      extensions,
      g.YAS_EXTENSION_LIMIT_MAX_OBJECT_STAGES_PER_SESSION,
    ),
    maxFollowsPerSession: limit32(
      extensions,
      g.YAS_EXTENSION_LIMIT_MAX_FOLLOWS_PER_SESSION,
    ),
    maxRunningAttempts: limit32(
      extensions,
      g.YAS_EXTENSION_LIMIT_MAX_RUNNING_ATTEMPTS,
    ),
    maxMemoryBytes: limit64(extensions, g.YAS_EXTENSION_LIMIT_MAX_MEMORY_BYTES),
    maxJobBytes: limit64(extensions, g.YAS_EXTENSION_LIMIT_MAX_JOB_BYTES),
    maxMutationReplays: limit32(
      extensions,
      g.YAS_EXTENSION_LIMIT_MAX_MUTATION_REPLAYS,
    ),
  };
  validateFamilyLimits(value);
  return value;
}

export class YasExtensionCatalog {
  private current = new Map<bigint, YasExtensionRecord>();
  private currentRetention: YasStateCatalogueRetention<bigint>;
  private staging: Map<bigint, YasExtensionRecord> | null = null;
  private stagingRetention: YasStateCatalogueRetention<bigint> | null = null;
  private subscription: YasStateSubscription | null = null;
  private revision = 0n;
  private listeners = new Set<(snapshot: YasExtensionSnapshot) => void>();
  private readonly snapshotRejectors = new Set<(error: unknown) => void>();
  private readonly removeInvalidation: () => void;
  private pendingWatch: Promise<void> | null = null;
  private pendingWatchCancel: ((error: unknown) => void) | null = null;
  private epoch = 0;
  private disposed = false;

  constructor(
    private readonly connection: YasConnection,
    private readonly onSubscriptionDrop: () => void = () => undefined,
    private readonly maxDefinitions: () => number = () =>
      g.YAS_EXTENSION_MAX_DEFINITIONS,
  ) {
    this.currentRetention =
      YasStateCatalogueRetention.forConnection(connection);
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family === undefined || family === g.YAS_FAMILY_EXTENSION) {
        this.epoch++;
        this.subscription = null;
        const error = new YasProtocolError(
          "Extension catalogue was invalidated",
        );
        this.pendingWatchCancel?.(error);
        this.cancelSnapshots(error);
        this.resetLocal();
      }
    });
  }

  get snapshot(): YasExtensionSnapshot {
    return { revision: this.revision, definitions: [...this.current.values()] };
  }

  subscribe(listener: (snapshot: YasExtensionSnapshot) => void): () => void {
    this.assertOpen();
    this.listeners.add(listener);
    try {
      listener(this.snapshot);
    } catch {
      // One observer cannot block catalogue delivery or cleanup.
    }
    return () => this.listeners.delete(listener);
  }

  async firstSnapshot(
    options: YasWatchOptions = {},
  ): Promise<YasExtensionSnapshot> {
    this.assertOpen();
    if (this.revision !== 0n && this.subscription?.active) return this.snapshot;
    let remove: (() => void) | undefined;
    let rejectSnapshot: ((error: unknown) => void) | undefined;
    const result = new Promise<YasExtensionSnapshot>((resolve, reject) => {
      let settled = false;
      const finish = (snapshot?: YasExtensionSnapshot, error?: unknown) => {
        if (settled) return;
        settled = true;
        remove?.();
        if (rejectSnapshot) this.snapshotRejectors.delete(rejectSnapshot);
        if (error !== undefined) reject(error);
        else resolve(snapshot!);
      };
      rejectSnapshot = (error) => finish(undefined, error);
      this.snapshotRejectors.add(rejectSnapshot);
      remove = this.subscribe((snapshot) => {
        if (snapshot.revision === 0n) return;
        finish(snapshot);
      });
    });
    try {
      return await Promise.race([
        result,
        this.watch(options).then(() => result),
      ]);
    } catch (error) {
      remove?.();
      if (rejectSnapshot) this.snapshotRejectors.delete(rejectSnapshot);
      throw error;
    }
  }

  async watch(options: YasWatchOptions = {}): Promise<void> {
    this.assertOpen();
    if (this.subscription?.active) return;
    if (this.pendingWatch) return this.pendingWatch;
    this.subscription = null;
    this.resetLocal();
    const epoch = this.epoch;
    const watched = YasStateSubscription.watch(
      this.connection,
      g.YAS_FAMILY_EXTENSION,
      g.YAS_EXTENSION_WATCH,
      g.YAS_EXTENSION_UNWATCH,
      g.YAS_EXTENSION_STATE,
      g.YAS_EXTENSION_STATE_ACK,
      options,
      (batch) => {
        if (!this.disposed && epoch === this.epoch) this.apply(batch);
      },
    ).then(async (subscription) => {
      if (this.disposed || epoch !== this.epoch) {
        await subscription.unwatch().catch(() => undefined);
        throw new YasProtocolError("Extension catalogue watch was cancelled");
      }
      this.subscription = subscription;
    });
    const cancelled = new Promise<never>((_, reject) => {
      this.pendingWatchCancel = reject;
    });
    const pending = Promise.race([watched, cancelled]);
    this.pendingWatch = pending;
    try {
      await pending;
    } finally {
      if (this.pendingWatch === pending) this.pendingWatch = null;
      if (this.pendingWatchCancel) this.pendingWatchCancel = null;
    }
  }

  async unwatch(): Promise<void> {
    this.assertOpen();
    this.epoch++;
    this.pendingWatchCancel?.(
      new YasProtocolError("Extension catalogue watch was cancelled"),
    );
    const subscription = this.subscription;
    this.subscription = null;
    this.resetLocal();
    await subscription?.unwatch();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.epoch++;
    this.removeInvalidation();
    const subscription = this.subscription;
    this.subscription = null;
    const error = new YasProtocolError("Extension catalogue is disposed");
    this.pendingWatchCancel?.(error);
    this.cancelSnapshots(error);
    this.resetLocal();
    this.listeners.clear();
    void subscription?.unwatch().catch(() => undefined);
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
        throw new YasProtocolError("Extension snapshot records without begin");
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
        throw new YasProtocolError("Extension snapshot end without begin");
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
      let next: Map<bigint, YasExtensionRecord>;
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
    definitions: ReadonlyMap<bigint, YasExtensionRecord>,
  ): void {
    if (definitions.size > this.maxDefinitions())
      throw new YasProtocolError(
        "Extension catalogue exceeds negotiated definition limit",
      );
  }

  private applyRecords(
    target: Map<bigint, YasExtensionRecord>,
    retention: YasStateCatalogueRetention<bigint>,
    records: readonly YasTypedRecord[],
  ): void {
    const originals = new Map<bigint, YasExtensionRecord | null>();
    const remember = (key: bigint) => {
      if (!originals.has(key)) originals.set(key, target.get(key) ?? null);
    };
    try {
      for (const action of records) {
        if (
          action.kind === YAS_STATE_ADD ||
          action.kind === YAS_STATE_REPLACE
        ) {
          const decoded = decodeExtensionRecord(action.body);
          const encoded = encodeExtensionRecord(decoded);
          const value = decodeExtensionRecord(encoded);
          const exists = target.has(value.extensionHandle);
          if ((action.kind === YAS_STATE_ADD) === exists)
            throw new YasProtocolError(
              "Extension ADD/REPLACE precondition failed",
            );
          remember(value.extensionHandle);
          retention.upsert(
            value.extensionHandle,
            Math.max(encoded.length, estimateStateRetainedBytes(value)),
          );
          target.set(value.extensionHandle, value);
        } else if (action.kind === YAS_STATE_REMOVE) {
          const removed = decodeExtensionRemovedRecord(action.body);
          const previous = target.get(removed.extensionHandle);
          if (!previous || previous.generation !== removed.generation)
            throw new YasProtocolError("Extension REMOVE precondition failed");
          remember(removed.extensionHandle);
          retention.remove(removed.extensionHandle);
          target.delete(removed.extensionHandle);
        } else
          throw new YasProtocolError("unsupported Extension state record kind");
      }
      this.validateCatalog(target);
    } catch (error) {
      for (const key of originals.keys()) retention.remove(key);
      for (const [key, original] of originals) {
        if (original) {
          retention.upsert(
            key,
            Math.max(
              encodeExtensionRecord(original).length,
              estimateStateRetainedBytes(original),
            ),
          );
          target.set(key, original);
        } else target.delete(key);
      }
      throw error;
    }
  }

  private resetLocal(): void {
    this.onSubscriptionDrop();
    this.subscription = null;
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
    for (const listener of this.listeners) {
      try {
        listener(snapshot);
      } catch {
        // One observer cannot block catalogue delivery or cleanup.
      }
    }
  }

  private cancelSnapshots(error: unknown): void {
    for (const reject of [...this.snapshotRejectors]) reject(error);
    this.snapshotRejectors.clear();
  }

  private assertOpen(): void {
    if (this.disposed)
      throw new YasProtocolError("Extension catalogue is disposed");
  }
}

export class YasExtensionClient {
  readonly catalog: YasExtensionCatalog;
  private readonly transfers;
  private readonly follows = new Set<YasExtensionFollowStream>();
  private readonly stagingUploads = new Map<bigint, YasExtensionObjectStage>();
  private readonly objectBeginOperations = new Map<
    string,
    YasExtensionObjectBeginOperation
  >();
  private readonly pendingObjectBeginOperations = new Map<
    string,
    YasExtensionObjectBeginOperation
  >();
  private readonly attemptContextRemovers = new Set<() => void>();
  private readonly pendingCancels = new Set<(error: unknown) => void>();
  private readonly removeInvalidation: () => void;
  private epoch = 0;
  private disposed = false;

  constructor(readonly connection: YasConnection) {
    connection.family(g.YAS_FAMILY_EXTENSION, g.YAS_EXTENSION_VERSION);
    connection.registerFamilyLimitValidator(
      g.YAS_FAMILY_EXTENSION,
      extensionLimitsFromExtensions,
    );
    this.catalog = new YasExtensionCatalog(
      connection,
      () => this.dropFollows(),
      () =>
        negotiatedStateLimitU32(
          connection,
          g.YAS_FAMILY_EXTENSION,
          g.YAS_EXTENSION_VERSION,
          g.YAS_EXTENSION_LIMIT_MAX_DEFINITIONS,
          g.YAS_EXTENSION_MAX_DEFINITIONS,
        ),
    );
    this.transfers = transfersFor(connection);
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family === undefined || family === g.YAS_FAMILY_EXTENSION) {
        this.epoch++;
        this.dropStagingUploads();
        this.retireObjectBeginOperations();
        const error = new YasProtocolError(
          "YAS Extension client was invalidated",
        );
        for (const cancel of [...this.pendingCancels]) cancel(error);
        this.pendingCancels.clear();
        this.dropFollows();
      }
    });
  }

  list(options: YasWatchOptions = {}): Promise<YasExtensionSnapshot> {
    return this.catalog.firstSnapshot(options);
  }

  beginObject(
    value: YasExtensionObjectBegin,
  ): Promise<YasExtensionObjectUpload | null> {
    this.assertOpen();
    this.pruneExpiredObjectUploads();
    const payload = encodeExtensionObjectBegin(value);
    const operationKey = byteKey(value.operationId);
    const payloadKey = byteKey(payload);
    let operation =
      this.objectBeginOperations.get(operationKey) ??
      this.pendingObjectBeginOperations.get(operationKey);
    if (operation) {
      if (operation.payloadKey !== payloadKey)
        throw new YasProtocolError(
          "Extension OBJECT_BEGIN operation ID was reused with a different payload",
        );
      if (operation.pending) return operation.pending;
      if (operation.hasResult) {
        if (operation.result === null) return Promise.resolve(null);
        const stage = this.stagingUploads.get(operation.result.stagingHandle);
        if (stage?.upload === operation.result)
          return Promise.resolve(operation.result);
        operation.result = null;
        operation.hasResult = false;
      }
    } else {
      this.ensureObjectBeginReplaySlot(operationKey);
      operation = {
        payloadKey,
        pending: null,
        result: null,
        hasResult: false,
        retainPayload: false,
      };
      this.pendingObjectBeginOperations.set(operationKey, operation);
    }
    if (operation.retainPayload) this.ensureObjectBeginReplaySlot(operationKey);
    const epoch = this.epoch;
    let request: Promise<YasExtensionObjectUpload | null>;
    try {
      request = this.connection
        .requestDecoded(
          g.YAS_FAMILY_EXTENSION,
          g.YAS_EXTENSION_OBJECT_BEGIN,
          payload,
          (body) =>
            this.installObjectBegin(
              decodeExtensionObjectBeginResult(body),
              operationKey,
              operation,
              epoch,
            ),
        )
        .then((result) =>
          result === null || "transfer" in result
            ? result
            : this.installObjectBegin(
                result as YasExtensionObjectBeginResult,
                operationKey,
                operation,
                epoch,
              ),
        );
    } catch (error) {
      if (!operation.hasResult && !operation.retainPayload)
        this.pendingObjectBeginOperations.delete(operationKey);
      throw error;
    }
    let pending!: Promise<YasExtensionObjectUpload | null>;
    pending = this.runOwned(request)
      .then((result) => {
        if (this.disposed || epoch !== this.epoch)
          throw new YasProtocolError(
            "Extension OBJECT_BEGIN completed after disposal or family invalidation",
          );
        return result;
      })
      .finally(() => {
        if (operation.pending !== pending) return;
        operation.pending = null;
        if (
          !operation.hasResult &&
          !operation.retainPayload &&
          this.pendingObjectBeginOperations.get(operationKey) === operation
        )
          this.pendingObjectBeginOperations.delete(operationKey);
      });
    operation.pending = pending;
    return pending;
  }

  async uploadObject(
    value: YasExtensionObjectBegin,
    bytes: Uint8Array,
    commitOperationId: Uint8Array,
  ): Promise<void> {
    this.assertOpen();
    if (BigInt(bytes.length) !== value.byteLength)
      throw new YasProtocolError(
        "Extension object bytes do not match declared length",
      );
    const upload = await this.beginObject(value);
    if (!upload) return;
    try {
      await upload.transfer.write(bytes);
      upload.transfer.closeWrite();
      await this.commitObject({
        stagingHandle: upload.stagingHandle,
        operationId: commitOperationId,
        contentHash: value.contentHash,
        byteLength: value.byteLength,
      });
    } catch (error) {
      upload.transfer.reset();
      this.retireObjectStage(upload.stagingHandle, false, upload);
      throw error;
    }
  }

  async commitObject(value: YasExtensionObjectCommit): Promise<void> {
    this.assertOpen();
    this.pruneExpiredObjectUploads();
    try {
      await this.connection.request(
        g.YAS_FAMILY_EXTENSION,
        g.YAS_EXTENSION_OBJECT_COMMIT,
        encodeExtensionObjectCommit(value),
      );
      this.retireObjectStage(value.stagingHandle, false);
    } catch (error) {
      if (
        error instanceof YasResultError &&
        error.status === g.YAS_STATUS_NOT_FOUND
      )
        this.retireObjectStage(value.stagingHandle, false);
      throw error;
    }
  }

  deploy(value: YasExtensionDeploy): Promise<YasExtensionDefinitionIdentity> {
    this.assertOpen();
    return this.connection.requestDecoded(
      g.YAS_FAMILY_EXTENSION,
      g.YAS_EXTENSION_DEPLOY,
      encodeExtensionDeploy(value),
      decodeExtensionDefinitionIdentity,
    );
  }

  control(value: YasExtensionControl): Promise<YasExtensionDefinitionIdentity> {
    this.assertOpen();
    return this.connection.requestDecoded(
      g.YAS_FAMILY_EXTENSION,
      g.YAS_EXTENSION_CONTROL,
      encodeExtensionControl(value),
      decodeExtensionDefinitionIdentity,
    );
  }

  follow(
    value: Omit<YasExtensionFollowRequest, "initialReceiveCredit">,
    initialReceiveCredit = 1024n * 1024n,
  ): Promise<YasExtensionFollowStream> {
    this.assertOpen();
    const epoch = this.epoch;
    return this.runOwned(
      this.performFollow(value, initialReceiveCredit, epoch),
    );
  }

  private async performFollow(
    value: Omit<YasExtensionFollowRequest, "initialReceiveCredit">,
    initialReceiveCredit: bigint,
    epoch: number,
  ): Promise<YasExtensionFollowStream> {
    const lease = this.transfers.reserveReceiveCredit(initialReceiveCredit, 1n);
    let accepted = false;
    try {
      const result = await this.connection.requestDecoded(
        g.YAS_FAMILY_EXTENSION,
        g.YAS_EXTENSION_FOLLOW,
        encodeExtensionFollow({ ...value, initialReceiveCredit: lease.bytes }),
        decodeExtensionFollowResult,
      );
      const transfer = this.transfers.acceptServerDescriptor(
        result.descriptor,
        lease,
      );
      accepted = true;
      if (this.disposed || epoch !== this.epoch) {
        transfer.reset();
        throw new YasProtocolError("Extension FOLLOW completed after disposal");
      }
      const stream: YasExtensionFollowStream = {
        attempt: result.attempt,
        firstSequence: result.firstSequence,
        throughSequence: result.throughSequence,
        transfer,
        async read() {
          const message = await transfer.readMessage();
          return message === null ? null : decodeExtensionOutputBatch(message);
        },
        reset() {
          transfer.reset();
        },
      };
      this.follows.add(stream);
      void transfer.closed.then(
        () => this.follows.delete(stream),
        () => this.follows.delete(stream),
      );
      return stream;
    } catch (error) {
      if (!accepted) lease.release();
      throw error;
    }
  }

  discoverCommands(
    value: YasExtensionDiscoverCommands,
  ): Promise<YasExtensionCommandPage> {
    this.assertOpen();
    return this.connection.requestDecoded(
      g.YAS_FAMILY_EXTENSION,
      g.YAS_EXTENSION_DISCOVER_COMMANDS,
      encodeExtensionDiscoverCommands(value),
      decodeExtensionCommandPage,
    );
  }

  private dropFollows(): void {
    for (const follow of this.follows) {
      try {
        follow.reset();
      } catch {
        // The physical session may already have closed the Transfer.
      }
    }
    this.follows.clear();
  }

  private installObjectBegin(
    result: YasExtensionObjectBeginResult,
    operationKey: string,
    operation: YasExtensionObjectBeginOperation,
    epoch: number,
  ): YasExtensionObjectUpload | null {
    if (this.disposed || epoch !== this.epoch) {
      this.resetUnownedObjectResult(result);
      throw new YasProtocolError(
        "Extension OBJECT_BEGIN completed after disposal or family invalidation",
      );
    }
    if (
      result.disposition === g.YAS_EXTENSION_OBJECT_UPLOAD &&
      this.stagingUploads.has(result.stagingHandle)
    ) {
      // Resetting a descriptor for a reused staging handle could discard the
      // pre-existing stage. Session teardown retires the malicious Result.
      throw new YasProtocolError("Extension staging handle was reused");
    }
    if (operation.retainPayload && !operation.hasResult) {
      this.resetUnownedObjectResult(result);
      throw new YasProtocolError(
        "Extension OBJECT_BEGIN replayed a retired stage instead of STALE",
      );
    }
    if (result.disposition === g.YAS_EXTENSION_OBJECT_ALREADY_PRESENT) {
      operation.result = null;
      operation.hasResult = true;
      operation.retainPayload = true;
      if (!this.retainObjectBeginOperation(operationKey, operation))
        throw new YasProtocolError(
          "Extension OBJECT_BEGIN replay ledger overflowed",
        );
      return null;
    }
    const descriptor = result.descriptor!;
    const expiresServerNs = descriptor.uploadStage!.expiresServerNs;
    if (this.connection.nanosecondsUntilServerTime(expiresServerNs) === 0n) {
      this.resetUnownedObjectResult(result);
      throw new YasProtocolError(
        "Extension OBJECT_BEGIN returned an expired upload stage",
      );
    }
    const transfer = this.transfers.acceptServerUploadDescriptor(descriptor);
    const upload: YasExtensionObjectUpload = {
      stagingHandle: result.stagingHandle,
      transfer,
      extensions: result.extensions,
    };
    const stage: YasExtensionObjectStage = {
      operationKey,
      upload,
      expiresServerNs,
      removeTerminalListener: () => undefined,
      removeResetListener: () => undefined,
    };
    this.stagingUploads.set(upload.stagingHandle, stage);
    operation.result = upload;
    operation.hasResult = true;
    operation.retainPayload = true;
    if (!this.retainObjectBeginOperation(operationKey, operation)) {
      this.stagingUploads.delete(upload.stagingHandle);
      operation.result = null;
      operation.hasResult = false;
      transfer.reset();
      throw new YasProtocolError(
        "Extension OBJECT_BEGIN replay ledger overflowed",
      );
    }
    stage.removeTerminalListener = transfer.subscribeTerminal(() =>
      this.tombstoneObjectStageOperation(stage),
    );
    stage.removeResetListener = transfer.subscribeReset(() =>
      this.retireObjectStage(upload.stagingHandle, false, upload),
    );
    if (this.stagingUploads.get(upload.stagingHandle) !== stage)
      throw new YasProtocolError(
        "Extension OBJECT_BEGIN completed with a retired upload stage",
      );
    return upload;
  }

  private resetUnownedObjectResult(
    result: YasExtensionObjectBeginResult,
  ): void {
    if (
      result.disposition !== g.YAS_EXTENSION_OBJECT_UPLOAD ||
      !result.descriptor ||
      this.stagingUploads.has(result.stagingHandle)
    )
      return;
    try {
      this.transfers.acceptServerUploadDescriptor(result.descriptor).reset();
    } catch {
      // Session teardown also retires an unaccepted or already-retired stage.
    }
  }

  private pruneExpiredObjectUploads(): void {
    for (const [stagingHandle, stage] of this.stagingUploads)
      if (
        this.connection.nanosecondsUntilServerTime(stage.expiresServerNs) === 0n
      )
        this.retireObjectStage(stagingHandle, true, stage.upload);
  }

  private retireObjectStage(
    stagingHandle: bigint,
    reset: boolean,
    expectedUpload?: YasExtensionObjectUpload,
  ): void {
    const stage = this.stagingUploads.get(stagingHandle);
    if (!stage || (expectedUpload && stage.upload !== expectedUpload)) return;
    this.stagingUploads.delete(stagingHandle);
    stage.removeTerminalListener();
    stage.removeTerminalListener = () => undefined;
    stage.removeResetListener();
    stage.removeResetListener = () => undefined;
    this.tombstoneObjectStageOperation(stage);
    if (reset) {
      try {
        stage.upload.transfer.reset();
      } catch {
        // Family invalidation may make Transfer cleanup unavailable.
      }
    }
  }

  private retireObjectBeginOperations(): void {
    for (const operation of this.objectBeginOperations.values()) {
      operation.pending = null;
      operation.result = null;
      operation.hasResult = false;
      operation.retainPayload = true;
    }
    for (const [operationKey, operation] of this.pendingObjectBeginOperations) {
      operation.pending = null;
      operation.result = null;
      operation.hasResult = false;
      operation.retainPayload = true;
      this.retainObjectBeginOperation(operationKey, operation);
    }
    this.pendingObjectBeginOperations.clear();
  }

  private tombstoneObjectStageOperation(stage: YasExtensionObjectStage): void {
    const operation = this.objectBeginOperations.get(stage.operationKey);
    if (operation?.result !== stage.upload) return;
    operation.result = null;
    operation.hasResult = false;
    operation.retainPayload = true;
  }

  private ensureObjectBeginReplaySlot(operationKey: string): void {
    let pinned = 0;
    for (const [key, operation] of this.objectBeginOperations) {
      if (key === operationKey) continue;
      if (
        operation.pending ||
        (operation.hasResult && operation.result !== null)
      )
        pinned++;
    }
    for (const key of this.pendingObjectBeginOperations.keys())
      if (key !== operationKey) pinned++;
    if (pinned + 1 > this.objectBeginReplayLimit())
      throw new YasResultError(
        g.YAS_STATUS_RESOURCE_EXHAUSTED,
        new Uint8Array(0),
        "Extension OBJECT_BEGIN replay ledger is full",
      );
  }

  private retainObjectBeginOperation(
    operationKey: string,
    operation: YasExtensionObjectBeginOperation,
  ): boolean {
    if (this.objectBeginOperations.get(operationKey) === operation) return true;
    const limit = this.objectBeginReplayLimit();
    let needed = this.objectBeginOperations.size - limit + 1;
    for (const [key, operation] of this.objectBeginOperations) {
      if (needed <= 0) break;
      if (
        !operation.pending &&
        operation.retainPayload &&
        (!operation.hasResult || operation.result === null)
      ) {
        this.objectBeginOperations.delete(key);
        needed--;
      }
    }
    if (needed > 0) return false;
    this.pendingObjectBeginOperations.delete(operationKey);
    this.objectBeginOperations.set(operationKey, operation);
    return true;
  }

  private objectBeginReplayLimit(): number {
    const extension = this.connection
      .family(g.YAS_FAMILY_EXTENSION, g.YAS_EXTENSION_VERSION)
      .limits.find(
        (candidate) =>
          candidate.tag === g.YAS_EXTENSION_LIMIT_MAX_MUTATION_REPLAYS,
      );
    if (!extension)
      throw new YasProtocolError(
        "required Extension mutation replay limit is absent",
      );
    const cursor = new YasCursor(extension.value);
    const value = cursor.u32("Extension mutation replay limit");
    cursor.end("Extension mutation replay limit");
    if (value === 0 || value > g.YAS_EXTENSION_MAX_MUTATION_REPLAYS)
      throw new YasProtocolError("invalid Extension mutation replay limit");
    return value;
  }

  private dropStagingUploads(): void {
    for (const [stagingHandle, stage] of [...this.stagingUploads]) {
      try {
        this.retireObjectStage(stagingHandle, true, stage.upload);
      } catch {
        // The shared Transfer registry may already be invalidated.
      }
    }
  }

  onAttemptContext(
    listener: (context: YasExtensionAttemptContext) => void,
  ): () => void {
    this.assertOpen();
    const removeRemote = this.connection.onEvent(
      g.YAS_FAMILY_EXTENSION,
      g.YAS_EXTENSION_ATTEMPT_CONTEXT,
      ({ payload }) => {
        try {
          listener(decodeExtensionAttemptContext(payload));
        } catch {
          // One observer cannot fail Event dispatch for its siblings.
        }
      },
    );
    let active = true;
    const remove = () => {
      if (!active) return;
      active = false;
      removeRemote();
      this.attemptContextRemovers.delete(remove);
    };
    this.attemptContextRemovers.add(remove);
    return remove;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.epoch++;
    const error = new YasProtocolError("YAS Extension client was disposed");
    for (const cancel of [...this.pendingCancels]) cancel(error);
    this.pendingCancels.clear();
    this.removeInvalidation();
    for (const remove of [...this.attemptContextRemovers]) remove();
    this.catalog.dispose();
    this.dropFollows();
    this.dropStagingUploads();
    this.retireObjectBeginOperations();
    this.objectBeginOperations.clear();
    this.pendingObjectBeginOperations.clear();
  }

  private assertOpen(): void {
    if (this.disposed)
      throw new YasProtocolError("Extension client is disposed");
  }

  private runOwned<T>(operation: Promise<T>): Promise<T> {
    let cancel!: (error: unknown) => void;
    const cancelled = new Promise<never>((_, reject) => {
      cancel = reject;
    });
    this.pendingCancels.add(cancel);
    return Promise.race([operation, cancelled]).finally(() => {
      this.pendingCancels.delete(cancel);
    });
  }
}

function byteKey(value: Uint8Array): string {
  let key = "";
  for (const byte of value) key += byte.toString(16).padStart(2, "0");
  return key;
}

function decodeExit(cursor: YasCursor): YasExtensionExitRecord {
  const kind = cursor.u8("Extension exit kind");
  requireZero(cursor.take(3, "Extension exit reserved"), "Extension exit");
  const value = {
    kind,
    code: cursor.i32("Extension exit code"),
    attempt: cursor.u64("Extension exit attempt"),
    serverNs: cursor.u64("Extension exit time"),
    detail: cursor.utf8U32("Extension exit detail"),
    extensions: decodeExtensions(cursor, new Set(), "Extension exit record"),
  };
  validateExit(value);
  return value;
}

function decodeCommandRecord(cursor: YasCursor): YasExtensionCommandRecord {
  const value = {
    extensionHandle: cursor.u64("Extension handle"),
    generation: cursor.u64("Extension generation"),
    definitionRevision: cursor.u64("Extension definition revision"),
    contentHash: new Uint8Array(cursor.take(32, "Extension object hash")),
    listenerHandle: cursor.u64("Extension listener handle"),
    listenerGeneration: cursor.u64("Extension listener generation"),
    name: cursor.utf8U16("Extension name"),
    listenerName: cursor.utf8U16("Extension listener name"),
    descriptor: cursor.utf8U32("Extension command descriptor"),
    extensions: decodeExtensions(cursor, new Set(), "Extension command record"),
  };
  validateCommandRecord(value);
  return value;
}

function validateRuntimeLimits(value: YasExtensionRuntimeLimits): void {
  if (
    value.memoryBytes > BigInt(g.YAS_EXTENSION_MAX_MEMORY_BYTES) ||
    value.stackBytes > BigInt(g.YAS_EXTENSION_MAX_STACK_BYTES) ||
    !within(value.maxActiveJobs, g.YAS_EXTENSION_MAX_ACTIVE_JOBS, true) ||
    !within(value.maxPendingJobs, g.YAS_EXTENSION_MAX_PENDING_JOBS, true) ||
    value.maxJobBytes > BigInt(g.YAS_EXTENSION_MAX_JOB_BYTES)
  )
    throw new YasProtocolError("invalid Extension runtime limits");
  encodeKnownExtensions(value.extensions, "Extension runtime limits");
}

function validateObjectBeginResult(value: YasExtensionObjectBeginResult): void {
  if (value.disposition === g.YAS_EXTENSION_OBJECT_ALREADY_PRESENT) {
    if (value.stagingHandle !== 0n || value.descriptor)
      throw new YasProtocolError(
        "invalid already-present Extension object Result",
      );
  } else if (value.disposition === g.YAS_EXTENSION_OBJECT_UPLOAD) {
    requireHandle(value.stagingHandle, "Extension staging handle");
    if (!value.descriptor)
      throw new YasProtocolError("missing Extension upload descriptor");
    validateObjectDescriptor(value.descriptor);
    requireTransferUploadStage(
      value.descriptor,
      value.stagingHandle,
      "Extension object descriptor",
    );
  } else throw new YasProtocolError("unknown Extension object disposition");
  encodeKnownExtensions(value.extensions, "Extension OBJECT_BEGIN Result");
}

function validateDeploy(value: YasExtensionDeploy): void {
  requireOperationId(value.operationId, "Extension DEPLOY");
  const creating =
    value.expectedExtensionHandle === 0n &&
    value.expectedGeneration === 0n &&
    value.expectedDefinitionRevision === 0n;
  if (!creating) {
    if (
      value.expectedExtensionHandle === 0n ||
      value.expectedGeneration === 0n ||
      value.expectedDefinitionRevision === 0n
    )
      throw new YasProtocolError("partial Extension DEPLOY identity CAS");
    validateIdentity(value.expectedExtensionHandle, value.expectedGeneration);
    requireRevision(
      value.expectedDefinitionRevision,
      "Extension expected definition revision",
    );
  }
  validateDefinitionFlags(value.flags);
  validateRuntime(value.runtime, true);
  if (
    value.restartPolicy < g.YAS_EXTENSION_RESTART_NEVER ||
    value.restartPolicy > g.YAS_EXTENSION_RESTART_ALWAYS
  )
    throw new YasProtocolError("invalid Extension restart policy");
  validateName(
    value.name,
    Boolean(value.flags & g.YAS_EXTENSION_DEFINITION_PERSISTENT),
  );
  requireHash(value.contentHash, "Extension object");
  validateArgv(value.argv);
  validateRuntimeLimits(value.runtimeLimits);
  const preserveArgv = value.extensions?.find(
    (extension) => extension.tag === g.YAS_EXTENSION_DEPLOY_PRESERVE_ARGV_TAG,
  );
  if (
    preserveArgv &&
    (creating || preserveArgv.value.length !== 0 || value.argv.length !== 0)
  )
    throw new YasProtocolError("invalid Extension preserve-argv extension");
  encodeDeployExtensions(value.extensions);
}

function validateAttemptContext(value: YasExtensionAttemptContext): void {
  validateIdentity(value.extensionHandle, value.generation);
  requireRevision(value.definitionRevision, "Extension definition revision");
  requireRevision(value.attempt, "Extension attempt");
  if (value.taskId === 0) throw new YasProtocolError("zero Extension task ID");
  validateDefinitionFlags(value.flags);
  validateRuntime(value.runtime, false);
  requireHash(value.contentHash, "Extension object");
  validateName(
    value.name,
    Boolean(value.flags & g.YAS_EXTENSION_DEFINITION_PERSISTENT),
  );
  validateArgv(value.argv);
  encodeKnownExtensions(value.extensions, "Extension attempt context");
}

function validateExtensionRecord(value: YasExtensionRecord): void {
  validateIdentity(value.extensionHandle, value.generation);
  requireRevision(value.definitionRevision, "Extension definition revision");
  if (
    value.phase < g.YAS_EXTENSION_PHASE_NEED_OBJECT ||
    value.phase > g.YAS_EXTENSION_PHASE_STOPPING
  )
    throw new YasProtocolError("invalid Extension phase");
  validateRuntime(
    value.runtime,
    value.phase === g.YAS_EXTENSION_PHASE_NEED_OBJECT,
  );
  if (
    value.runtime === g.YAS_EXTENSION_RUNTIME_AUTO &&
    value.phase !== g.YAS_EXTENSION_PHASE_NEED_OBJECT
  )
    throw new YasProtocolError("Extension AUTO runtime outside NEED_OBJECT");
  if (
    value.restartPolicy < g.YAS_EXTENSION_RESTART_NEVER ||
    value.restartPolicy > g.YAS_EXTENSION_RESTART_ALWAYS
  )
    throw new YasProtocolError("invalid Extension restart policy");
  validateDefinitionFlags(value.flags);
  if (value.lastRunningAttempt > value.attempt)
    throw new YasProtocolError("invalid Extension attempt history");
  if (
    value.phase === g.YAS_EXTENSION_PHASE_RUNNING &&
    (value.attempt === 0n || value.taskId === 0)
  )
    throw new YasProtocolError("invalid running Extension identity");
  if (
    (value.phase === g.YAS_EXTENSION_PHASE_BACKOFF) !==
    (value.nextStartUnixMs !== 0n)
  )
    throw new YasProtocolError("invalid Extension backoff deadline");
  if (value.nextStartUnixMs > BigInt(g.YAS_EXTENSION_MAX_NEXT_START_UNIX_MS))
    throw new YasProtocolError("invalid Extension backoff deadline");
  requireHash(value.contentHash, "Extension object");
  validateName(
    value.name,
    Boolean(value.flags & g.YAS_EXTENSION_DEFINITION_PERSISTENT),
  );
  if (value.lastExit) {
    validateExit(value.lastExit);
    if (value.lastExit.attempt > value.attempt)
      throw new YasProtocolError("Extension exit exceeds current attempt");
  }
  validateRuntimeLimits(value.runtimeLimits);
  encodeKnownExtensions(value.extensions, "Extension state record");
}

function validateExit(value: YasExtensionExitRecord): void {
  if (
    value.kind < g.YAS_EXTENSION_EXIT_RETURNED ||
    value.kind > g.YAS_EXTENSION_EXIT_RESOURCE_LIMIT ||
    value.attempt === 0n ||
    new TextEncoder().encode(value.detail).length > 4096 ||
    (value.kind !== g.YAS_EXTENSION_EXIT_RETURNED && value.code !== 0)
  )
    throw new YasProtocolError("invalid Extension exit record");
  encodeKnownExtensions(value.extensions, "Extension exit record");
}

function validateOutputRecord(value: YasExtensionOutputRecord): void {
  if (
    value.kind < g.YAS_EXTENSION_OUTPUT_STDOUT ||
    value.kind > g.YAS_EXTENSION_OUTPUT_GAP
  )
    throw new YasProtocolError("invalid Extension output kind");
  if (
    value.data.length > g.YAS_EXTENSION_MAX_OUTPUT_RECORD_BYTES ||
    (value.kind === g.YAS_EXTENSION_OUTPUT_GAP && value.data.length !== 8)
  )
    throw new YasProtocolError("invalid Extension output record");
}

function validateCommandRecord(value: YasExtensionCommandRecord): void {
  validateIdentity(value.extensionHandle, value.generation);
  validateIdentity(value.listenerHandle, value.listenerGeneration);
  requireRevision(value.definitionRevision, "Extension definition revision");
  requireHash(value.contentHash, "Extension object");
  validateName(value.name, true);
  validateName(value.listenerName, true);
  const descriptorLength = new TextEncoder().encode(value.descriptor).length;
  if (
    descriptorLength === 0 ||
    descriptorLength > g.YAS_EXTENSION_MAX_COMMAND_DESCRIPTOR_BYTES
  )
    throw new YasProtocolError("invalid Extension command descriptor");
  encodeKnownExtensions(value.extensions, "Extension command record");
}

function validateDefinitionFlags(flags: number): void {
  if (
    !Number.isInteger(flags) ||
    flags < 0 ||
    flags & ~g.YAS_EXTENSION_DEFINITION_FLAGS ||
    (flags & g.YAS_EXTENSION_DEFINITION_DESIRED_RUNNING &&
      !(flags & g.YAS_EXTENSION_DEFINITION_ENABLED)) ||
    (flags & g.YAS_EXTENSION_DEFINITION_PERSISTENT &&
      !(flags & g.YAS_EXTENSION_DEFINITION_DETACHED))
  )
    throw new YasProtocolError("invalid Extension definition flags");
}

function validateRuntime(runtime: number, allowAuto: boolean): void {
  if (
    !Number.isInteger(runtime) ||
    runtime <
      (allowAuto
        ? g.YAS_EXTENSION_RUNTIME_AUTO
        : g.YAS_EXTENSION_RUNTIME_WASMI) ||
    runtime > g.YAS_EXTENSION_RUNTIME_QUICKJS
  )
    throw new YasProtocolError("invalid Extension runtime");
}

function validateArgv(argv: readonly Uint8Array[]): void {
  if (argv.length > g.YAS_EXTENSION_MAX_ARGS)
    throw new YasProtocolError("too many Extension arguments");
  let total = 0;
  for (const argument of argv) {
    if (argument.length > g.YAS_EXTENSION_MAX_ARG_BYTES)
      throw new YasProtocolError("Extension argument exceeds its byte limit");
    total += argument.length;
  }
  if (total > g.YAS_EXTENSION_MAX_ARGUMENT_BYTES)
    throw new YasProtocolError("Extension arguments exceed their byte limit");
}

function validateName(name: string, required: boolean): void {
  const bytes = new TextEncoder().encode(name);
  if (
    (required && bytes.length === 0) ||
    bytes.length > g.YAS_EXTENSION_MAX_NAME_BYTES ||
    bytes.includes(0)
  )
    throw new YasProtocolError("invalid Extension name");
}

function validateIdentity(handle: bigint, generation: bigint): void {
  requireHandle(handle, "Extension handle");
  requireRevision(generation, "Extension generation");
}

function validateObjectDescriptor(descriptor: YasTransferDescriptor): void {
  validateSensitiveDescriptor(descriptor);
  if (
    descriptor.mode !== YAS_TRANSFER_MODE_BYTE ||
    descriptor.direction !== YAS_TRANSFER_RECEIVER_TO_SENDER ||
    descriptor.senderSendCredit !== 0n ||
    descriptor.receiverSendCredit === 0n ||
    descriptor.maxItemBytes !== 0n ||
    descriptor.contentKind !== g.YAS_EXTENSION_OBJECT_CONTENT_KIND
  )
    throw new YasProtocolError("invalid Extension object Transfer descriptor");
}

function validateFollowResult(value: YasExtensionFollowResult): void {
  if (value.attempt === 0n || value.firstSequence > value.throughSequence + 1n)
    throw new YasProtocolError("invalid Extension follow sequence range");
  const descriptor = value.descriptor;
  validateSensitiveDescriptor(descriptor);
  if (
    descriptor.mode !== YAS_TRANSFER_MODE_MESSAGE ||
    descriptor.direction !== YAS_TRANSFER_SENDER_TO_RECEIVER ||
    descriptor.receiverSendCredit !== 0n ||
    descriptor.maxItemBytes === 0n ||
    descriptor.maxItemBytes > BigInt(g.YAS_EXTENSION_MAX_OUTPUT_BATCH_BYTES) ||
    descriptor.contentKind !== g.YAS_EXTENSION_FOLLOW_CONTENT_KIND
  )
    throw new YasProtocolError("invalid Extension follow Transfer descriptor");
  encodeKnownExtensions(value.extensions, "Extension FOLLOW Result");
}

function validateSensitiveDescriptor(descriptor: YasTransferDescriptor): void {
  encodeTransferDescriptor(descriptor);
  const sensitive = descriptor.extensions.some(
    (extension) =>
      extension.tag === g.YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION &&
      extension.required &&
      extension.value.length === 0,
  );
  if (
    descriptor.contentFamily !== g.YAS_FAMILY_EXTENSION ||
    descriptor.contentVersion !== g.YAS_EXTENSION_VERSION ||
    !sensitive
  )
    throw new YasProtocolError("invalid Extension Transfer descriptor");
}

function decodeDescriptor(bytes: Uint8Array): YasTransferDescriptor {
  const cursor = new YasCursor(bytes);
  const descriptor = decodeTransferDescriptor(cursor);
  cursor.end("Extension Transfer descriptor");
  return descriptor;
}

function requireHash(value: Uint8Array, context: string): void {
  if (value.length !== 32)
    throw new YasProtocolError(`${context} hash is not 32 bytes`);
}

function requireOperationId(value: Uint8Array, context: string): void {
  if (value.length !== 16 || value.every((byte) => byte === 0))
    throw new YasProtocolError(`${context} operation ID is invalid`);
}

function requireHandle(value: bigint, context: string): void {
  if (value === 0n) throw new YasProtocolError(`${context} is zero`);
}

function requireRevision(value: bigint, context: string): void {
  if (value === 0n) throw new YasProtocolError(`${context} is zero`);
}

function requireObjectLength(value: bigint): void {
  if (value === 0n || value > BigInt(g.YAS_EXTENSION_MAX_OBJECT_BYTES))
    throw new YasProtocolError("invalid Extension object length");
}

function requireZero(value: Uint8Array, context: string): void {
  if (value.some((byte) => byte !== 0))
    throw new YasProtocolError(`${context} reserved bytes are nonzero`);
}

function encodeKnownExtensions(
  extensions: readonly YasExtension[] | undefined,
  context: string,
): Uint8Array {
  if (extensions?.some((extension) => extension.required))
    throw new YasProtocolError(
      `${context} contains an unknown required extension`,
    );
  return encodeExtensions(extensions);
}

function encodeDeployExtensions(
  extensions: readonly YasExtension[] | undefined,
): Uint8Array {
  if (
    extensions?.some(
      (extension) =>
        extension.required &&
        extension.tag !== g.YAS_EXTENSION_DEPLOY_PRESERVE_ARGV_TAG,
    )
  )
    throw new YasProtocolError(
      "Extension DEPLOY contains an unknown required extension",
    );
  return encodeExtensions(extensions);
}

function within(value: number, maximum: number, allowZero = false): boolean {
  return (
    Number.isInteger(value) && value >= (allowZero ? 0 : 1) && value <= maximum
  );
}

function limit32(extensions: readonly YasExtension[], tag: number): number {
  const extension = extensions.find((item) => item.tag === tag);
  if (!extension) throw new YasProtocolError("missing Extension family limit");
  const cursor = new YasCursor(extension.value);
  const value = cursor.u32("Extension family limit");
  cursor.end("Extension family limit");
  return value;
}

function limit64(extensions: readonly YasExtension[], tag: number): bigint {
  const extension = extensions.find((item) => item.tag === tag);
  if (!extension) throw new YasProtocolError("missing Extension family limit");
  const cursor = new YasCursor(extension.value);
  const value = cursor.u64("Extension family limit");
  cursor.end("Extension family limit");
  return value;
}

function validateFamilyLimits(value: YasExtensionLimits): void {
  if (
    !within(value.maxNameBytes, g.YAS_EXTENSION_MAX_NAME_BYTES) ||
    !within(value.maxArgs, g.YAS_EXTENSION_MAX_ARGS) ||
    value.maxArgumentBytes === 0n ||
    value.maxArgumentBytes > BigInt(g.YAS_EXTENSION_MAX_ARGUMENT_BYTES) ||
    value.maxObjectBytes === 0n ||
    value.maxObjectBytes > BigInt(g.YAS_EXTENSION_MAX_OBJECT_BYTES) ||
    !within(
      value.maxOutputRecordBytes,
      g.YAS_EXTENSION_MAX_OUTPUT_RECORD_BYTES,
    ) ||
    !within(
      value.maxCommandDescriptorBytes,
      g.YAS_EXTENSION_MAX_COMMAND_DESCRIPTOR_BYTES,
    ) ||
    !within(value.maxCommandRecords, g.YAS_EXTENSION_MAX_COMMAND_RECORDS) ||
    !within(value.maxDefinitions, g.YAS_EXTENSION_MAX_DEFINITIONS) ||
    !within(
      value.maxObjectStagesPerSession,
      g.YAS_EXTENSION_MAX_OBJECT_STAGES_PER_SESSION,
    ) ||
    !within(
      value.maxFollowsPerSession,
      g.YAS_EXTENSION_MAX_FOLLOWS_PER_SESSION,
    ) ||
    !within(value.maxRunningAttempts, g.YAS_EXTENSION_MAX_RUNNING_ATTEMPTS) ||
    value.maxMemoryBytes === 0n ||
    value.maxMemoryBytes > BigInt(g.YAS_EXTENSION_MAX_MEMORY_BYTES) ||
    value.maxJobBytes === 0n ||
    value.maxJobBytes > BigInt(g.YAS_EXTENSION_MAX_JOB_BYTES) ||
    !within(value.maxMutationReplays, g.YAS_EXTENSION_MAX_MUTATION_REPLAYS)
  )
    throw new YasProtocolError("invalid Extension family limits");
}
