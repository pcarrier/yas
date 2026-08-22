/** YAS process-wide binary event-journal family v1. */

import * as g from "./generated";
import type { YasConnection } from "./session";
import {
  YAS_TRANSFER_MODE_BYTE,
  YAS_TRANSFER_SENDER_TO_RECEIVER,
  decodeTransferDescriptor,
  encodeTransferDescriptor,
  transfersFor,
  type YasTransfer,
  type YasTransferDescriptor,
} from "./transfer";
import {
  YAS_MAX_DECODED_FRAME,
  YasCursor,
  YasDisconnectedError,
  YasProtocolError,
  YasResultError,
  YasWriter,
  decodeExtensions,
  encodeExtensions,
  equalBytes,
  type YasExtension,
} from "./wire";

export {
  YAS_EVENTS_DUMP,
  YAS_EVENTS_GAP,
  YAS_EVENTS_GET_CONFIG,
  YAS_EVENTS_LIST_RECORDINGS,
  YAS_EVENTS_RECORD,
  YAS_EVENTS_SET_CONFIG,
  YAS_EVENTS_START_RECORDING,
  YAS_EVENTS_START_STREAM,
  YAS_EVENTS_STOP_RECORDING,
  YAS_EVENTS_STOP_STREAM,
  YAS_EVENTS_STREAM_STOPPED,
  YAS_EVENTS_VERSION,
  YAS_FAMILY_EVENTS,
} from "./generated";

export interface YasEventsConfig {
  revision: bigint;
  capacity: bigint;
  used: bigint;
  recordCount: bigint;
  dropped: bigint;
  nextSequence: bigint;
  activations: readonly bigint[];
  extensions: readonly YasExtension[];
}

export interface YasEventsSetConfig {
  operationId: Uint8Array;
  expectedRevision: bigint;
  capacity: bigint;
  activations: readonly bigint[];
  extensions?: readonly YasExtension[];
}

export interface YasEventsDumpRequest {
  initialReceiveCredit: bigint;
  extensions?: readonly YasExtension[];
}

export interface YasEventsDumpResult {
  byteLength: bigint;
  contentHash: Uint8Array;
  descriptor: YasTransferDescriptor;
  extensions: readonly YasExtension[];
}

export interface YasEventsDump {
  bytes: Uint8Array;
  contentHash: Uint8Array;
}

export interface YasEventsStartStream {
  operationId: Uint8Array;
  history: boolean;
  startSequence: bigint;
  maxBatchBytes: number;
  extensions?: readonly YasExtension[];
}

export interface YasEventsStreamStarted {
  streamHandle: bigint;
  firstSequence: bigint;
  maxBatchBytes: number;
  extensions: readonly YasExtension[];
}

export interface YasEventsRecordingInfo {
  recordingHandle: bigint;
  state: number;
  history: boolean;
  append: boolean;
  records: bigint;
  bytes: bigint;
  lost: bigint;
  path: Uint8Array;
  error: string;
  extensions: readonly YasExtension[];
}

export interface YasEventsStartRecording {
  operationId: Uint8Array;
  history: boolean;
  append: boolean;
  path: Uint8Array;
  extensions?: readonly YasExtension[];
}

export interface YasEventRecord {
  sequence: bigint;
  monotonicNs: bigint;
  eventId: number;
  required: boolean;
  eventFlags: number;
  payload: Uint8Array;
}

export interface YasEventBatch {
  firstSequence: bigint;
  records: readonly YasEventRecord[];
}

export interface YasEventsRecordEvent {
  streamHandle: bigint;
  batch: YasEventBatch;
}

export interface YasEventsGap {
  streamHandle: bigint;
  lost: bigint;
  firstAvailableSequence: bigint;
}

export interface YasEventsStreamStopped {
  streamHandle: bigint;
  status: number;
  detail: string;
  extensions: readonly YasExtension[];
}

export type YasEventsStreamItem =
  | { type: "records"; batch: YasEventBatch }
  | { type: "gap"; lost: bigint; firstAvailableSequence: bigint }
  | { type: "stopped"; status: number; detail: string };

export type YasEventsHashBytes = (
  bytes: Uint8Array,
) => Uint8Array | Promise<Uint8Array>;

export function encodeEventsGetConfig(
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  rejectRequiredExtensions(extensions, "Events GET_CONFIG");
  return encodeExtensions(extensions);
}

export function decodeEventsGetConfig(bytes: Uint8Array): YasExtension[] {
  const cursor = new YasCursor(bytes);
  const extensions = decodeExtensions(cursor, new Set(), "Events GET_CONFIG");
  cursor.end("Events GET_CONFIG");
  return extensions;
}

export function encodeEventsConfig(value: YasEventsConfig): Uint8Array {
  validateConfig(value);
  const writer = new YasWriter()
    .u64(value.revision)
    .u64(value.capacity)
    .u64(value.used)
    .u64(value.recordCount)
    .u64(value.dropped)
    .u64(value.nextSequence);
  encodeActivations(writer, value.activations);
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeEventsConfig(bytes: Uint8Array): YasEventsConfig {
  const cursor = new YasCursor(bytes);
  const value = {
    revision: cursor.u64("Events configuration revision"),
    capacity: cursor.u64("Events ring capacity"),
    used: cursor.u64("Events retained bytes"),
    recordCount: cursor.u64("Events retained record count"),
    dropped: cursor.u64("Events ring overwrite count"),
    nextSequence: cursor.u64("Events next sequence"),
    activations: decodeActivations(cursor),
    extensions: decodeExtensions(cursor, new Set(), "Events config extensions"),
  };
  cursor.end("Events config");
  validateConfig(value);
  return value;
}

export function encodeEventsSetConfig(value: YasEventsSetConfig): Uint8Array {
  requireOperationId(value.operationId, "Events SET_CONFIG");
  validateCapacity(value.capacity);
  const writer = new YasWriter()
    .bytes(value.operationId)
    .u64(value.expectedRevision)
    .u64(value.capacity);
  encodeActivations(writer, value.activations);
  rejectRequiredExtensions(value.extensions ?? [], "Events SET_CONFIG");
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeEventsSetConfig(bytes: Uint8Array): YasEventsSetConfig {
  const cursor = new YasCursor(bytes);
  const value = {
    operationId: new Uint8Array(cursor.take(16, "Events operation ID")),
    expectedRevision: cursor.u64("Events expected configuration revision"),
    capacity: cursor.u64("Events ring capacity"),
    activations: decodeActivations(cursor),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Events SET_CONFIG extensions",
    ),
  };
  cursor.end("Events SET_CONFIG");
  encodeEventsSetConfig(value);
  return value;
}

export function encodeEventsDumpRequest(
  value: YasEventsDumpRequest,
): Uint8Array {
  if (value.initialReceiveCredit === 0n)
    throw new YasProtocolError("zero Events dump receive credit");
  rejectRequiredExtensions(value.extensions ?? [], "Events DUMP");
  return new YasWriter()
    .u64(value.initialReceiveCredit)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeEventsDumpRequest(
  bytes: Uint8Array,
): YasEventsDumpRequest {
  const cursor = new YasCursor(bytes);
  const value = {
    initialReceiveCredit: cursor.u64("Events dump receive credit"),
    extensions: decodeExtensions(cursor, new Set(), "Events DUMP extensions"),
  };
  cursor.end("Events DUMP");
  encodeEventsDumpRequest(value);
  return value;
}

export function encodeEventsDumpResult(value: YasEventsDumpResult): Uint8Array {
  validateHash(value.contentHash, "Events dump");
  validateDumpDescriptor(value.descriptor);
  rejectRequiredExtensions(value.extensions, "Events dump Result");
  return new YasWriter()
    .u64(value.byteLength)
    .bytes(value.contentHash)
    .bytesU32(encodeTransferDescriptor(value.descriptor))
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeEventsDumpResult(bytes: Uint8Array): YasEventsDumpResult {
  const cursor = new YasCursor(bytes);
  const byteLength = cursor.u64("Events dump length");
  const contentHash = new Uint8Array(cursor.take(32, "Events dump hash"));
  const descriptorCursor = cursor.sub(
    cursor.u32("Events dump Transfer length"),
    "Events dump Transfer",
  );
  const descriptor = decodeTransferDescriptor(descriptorCursor);
  descriptorCursor.end("Events dump Transfer");
  const value = {
    byteLength,
    contentHash,
    descriptor,
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Events dump Result extensions",
    ),
  };
  cursor.end("Events dump Result");
  encodeEventsDumpResult(value);
  return value;
}

export function encodeEventsStartStream(
  value: YasEventsStartStream,
): Uint8Array {
  requireOperationId(value.operationId, "Events START_STREAM");
  if (
    (!value.history && value.startSequence !== 0n) ||
    !Number.isInteger(value.maxBatchBytes) ||
    value.maxBatchBytes < 0 ||
    value.maxBatchBytes > g.YAS_EVENTS_MAX_LIVE_BATCH_BYTES
  )
    throw new YasProtocolError(
      "invalid Events stream history or batch options",
    );
  rejectRequiredExtensions(value.extensions ?? [], "Events START_STREAM");
  return new YasWriter()
    .bytes(value.operationId)
    .u16(value.history ? g.YAS_EVENTS_STREAM_HISTORY : 0)
    .u16(0)
    .u64(value.startSequence)
    .u32(value.maxBatchBytes)
    .u32(0)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeEventsStartStream(
  bytes: Uint8Array,
): YasEventsStartStream {
  const cursor = new YasCursor(bytes);
  const operationId = new Uint8Array(cursor.take(16, "Events operation ID"));
  const flags = cursor.u16("Events stream flags");
  if (flags & ~g.YAS_EVENTS_STREAM_FLAGS || cursor.u16("Events reserved") !== 0)
    throw new YasProtocolError("invalid Events stream flags or reserved field");
  const startSequence = cursor.u64("Events stream start sequence");
  const maxBatchBytes = cursor.u32("Events stream batch bytes");
  if (cursor.u32("Events stream reserved") !== 0)
    throw new YasProtocolError("Events stream reserved field is nonzero");
  const value = {
    operationId,
    history: Boolean(flags & g.YAS_EVENTS_STREAM_HISTORY),
    startSequence,
    maxBatchBytes,
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Events START_STREAM extensions",
    ),
  };
  cursor.end("Events START_STREAM");
  encodeEventsStartStream(value);
  return value;
}

export function encodeEventsStreamStarted(
  value: YasEventsStreamStarted,
): Uint8Array {
  requireHandle(value.streamHandle, "Events stream");
  if (
    value.maxBatchBytes === 0 ||
    value.maxBatchBytes > g.YAS_EVENTS_MAX_LIVE_BATCH_BYTES
  )
    throw new YasProtocolError("invalid Events stream batch size");
  rejectRequiredExtensions(value.extensions, "Events stream Result");
  return new YasWriter()
    .u64(value.streamHandle)
    .u64(value.firstSequence)
    .u32(value.maxBatchBytes)
    .u32(0)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeEventsStreamStarted(
  bytes: Uint8Array,
): YasEventsStreamStarted {
  const cursor = new YasCursor(bytes);
  const streamHandle = cursor.u64("Events stream handle");
  const firstSequence = cursor.u64("Events stream first sequence");
  const maxBatchBytes = cursor.u32("Events stream batch bytes");
  if (cursor.u32("Events stream reserved") !== 0)
    throw new YasProtocolError(
      "Events stream Result reserved field is nonzero",
    );
  const value = {
    streamHandle,
    firstSequence,
    maxBatchBytes,
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Events stream Result extensions",
    ),
  };
  cursor.end("Events stream Result");
  encodeEventsStreamStarted(value);
  return value;
}

export function encodeEventsStopStream(
  streamHandle: bigint,
  operationId: Uint8Array,
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  requireHandle(streamHandle, "Events stream");
  requireOperationId(operationId, "Events STOP_STREAM");
  rejectRequiredExtensions(extensions, "Events STOP_STREAM");
  return new YasWriter()
    .u64(streamHandle)
    .bytes(operationId)
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function decodeEventsStopStream(bytes: Uint8Array): {
  streamHandle: bigint;
  operationId: Uint8Array;
  extensions: readonly YasExtension[];
} {
  const cursor = new YasCursor(bytes);
  const value = {
    streamHandle: cursor.u64("Events stream handle"),
    operationId: new Uint8Array(cursor.take(16, "Events operation ID")),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Events STOP_STREAM extensions",
    ),
  };
  cursor.end("Events STOP_STREAM");
  encodeEventsStopStream(
    value.streamHandle,
    value.operationId,
    value.extensions,
  );
  return value;
}

export function encodeEventsStartRecording(
  value: YasEventsStartRecording,
): Uint8Array {
  requireOperationId(value.operationId, "Events START_RECORDING");
  validateNativePath(value.path);
  rejectRequiredExtensions(value.extensions ?? [], "Events START_RECORDING");
  const flags =
    (value.history ? g.YAS_EVENTS_RECORDING_HISTORY : 0) |
    (value.append ? g.YAS_EVENTS_RECORDING_APPEND : 0);
  return new YasWriter()
    .bytes(value.operationId)
    .u16(flags)
    .u16(0)
    .bytesU32(value.path)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeEventsStartRecording(
  bytes: Uint8Array,
): YasEventsStartRecording {
  const cursor = new YasCursor(bytes);
  const operationId = new Uint8Array(cursor.take(16, "Events operation ID"));
  const flags = cursor.u16("Events recording flags");
  if (
    flags & ~g.YAS_EVENTS_RECORDING_FLAGS ||
    cursor.u16("Events recording reserved") !== 0
  )
    throw new YasProtocolError(
      "invalid Events recording flags or reserved field",
    );
  const value = {
    operationId,
    history: Boolean(flags & g.YAS_EVENTS_RECORDING_HISTORY),
    append: Boolean(flags & g.YAS_EVENTS_RECORDING_APPEND),
    path: new Uint8Array(cursor.bytesU32("Events recording path")),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Events START_RECORDING extensions",
    ),
  };
  cursor.end("Events START_RECORDING");
  encodeEventsStartRecording(value);
  return value;
}

export function encodeEventsStopRecording(
  recordingHandle: bigint,
  operationId: Uint8Array,
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  requireHandle(recordingHandle, "Events recording");
  requireOperationId(operationId, "Events STOP_RECORDING");
  rejectRequiredExtensions(extensions, "Events STOP_RECORDING");
  return new YasWriter()
    .u64(recordingHandle)
    .bytes(operationId)
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function decodeEventsStopRecording(bytes: Uint8Array): {
  recordingHandle: bigint;
  operationId: Uint8Array;
  extensions: readonly YasExtension[];
} {
  const cursor = new YasCursor(bytes);
  const value = {
    recordingHandle: cursor.u64("Events recording handle"),
    operationId: new Uint8Array(cursor.take(16, "Events operation ID")),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Events STOP_RECORDING extensions",
    ),
  };
  cursor.end("Events STOP_RECORDING");
  encodeEventsStopRecording(
    value.recordingHandle,
    value.operationId,
    value.extensions,
  );
  return value;
}

export function encodeEventsRecordingInfo(
  value: YasEventsRecordingInfo,
): Uint8Array {
  const writer = new YasWriter();
  encodeRecordingInfoTo(writer, value);
  return writer.finish();
}

export function decodeEventsRecordingInfo(
  bytes: Uint8Array,
): YasEventsRecordingInfo {
  const cursor = new YasCursor(bytes);
  const value = decodeRecordingInfoFrom(cursor);
  cursor.end("Events recording info");
  return value;
}

export function encodeEventsRecordingList(
  recordings: readonly YasEventsRecordingInfo[],
): Uint8Array {
  if (recordings.length > g.YAS_EVENTS_MAX_RECORDINGS)
    throw new YasProtocolError("too many Events recordings");
  const writer = new YasWriter().u16(recordings.length).u16(0);
  for (const recording of recordings) encodeRecordingInfoTo(writer, recording);
  return writer.finish();
}

export function decodeEventsRecordingList(
  bytes: Uint8Array,
): YasEventsRecordingInfo[] {
  const cursor = new YasCursor(bytes);
  const count = cursor.u16("Events recording count");
  if (
    cursor.u16("Events recording-list reserved") !== 0 ||
    count > g.YAS_EVENTS_MAX_RECORDINGS ||
    count > Math.floor(cursor.remaining / 44)
  )
    throw new YasProtocolError("invalid Events recording count");
  const recordings: YasEventsRecordingInfo[] = [];
  for (let index = 0; index < count; index++)
    recordings.push(decodeRecordingInfoFrom(cursor));
  cursor.end("Events recording list");
  return recordings;
}

export function encodeEventsBatch(value: YasEventBatch): Uint8Array {
  validateBatch(value);
  const writer = new YasWriter()
    .u64(value.firstSequence)
    .u16(value.records.length)
    .u16(0);
  for (const record of value.records) {
    const flags = record.required ? g.YAS_EVENTS_RECORD_REQUIRED : 0;
    writer
      .u32(28 + record.payload.length)
      .u64(record.sequence)
      .u64(record.monotonicNs)
      .u32(record.eventId)
      .u16(flags)
      .u16(record.eventFlags)
      .bytes(record.payload);
  }
  return writer.finish();
}

export function decodeEventsBatch(bytes: Uint8Array): YasEventBatch {
  if (bytes.length > YAS_MAX_DECODED_FRAME)
    throw new YasProtocolError("Events batch exceeds the decoded-frame limit");
  const cursor = new YasCursor(bytes);
  const firstSequence = cursor.u64("Events first sequence");
  const count = cursor.u16("Events record count");
  if (
    cursor.u16("Events batch reserved") !== 0 ||
    count === 0 ||
    count > g.YAS_HARD_MAX_TYPED_RECORDS ||
    count > Math.floor(cursor.remaining / 28)
  )
    throw new YasProtocolError("invalid Events record count or reserved field");
  const records: YasEventRecord[] = [];
  for (let index = 0; index < count; index++) {
    const length = cursor.u32("Events record length");
    if (length < 28) throw new YasProtocolError("invalid Events record length");
    const recordCursor = cursor.sub(length - 4, "Events record");
    const sequence = recordCursor.u64("Events sequence");
    const monotonicNs = recordCursor.u64("Events monotonic time");
    const eventId = recordCursor.u32("Events event ID");
    const flags = recordCursor.u16("Events record flags");
    if (flags & ~g.YAS_EVENTS_RECORD_FLAGS_MASK)
      throw new YasProtocolError("invalid Events record flags");
    const record: YasEventRecord = {
      sequence,
      monotonicNs,
      eventId,
      required: Boolean(flags & g.YAS_EVENTS_RECORD_REQUIRED),
      eventFlags: recordCursor.u16("Events event flags"),
      payload: new Uint8Array(
        recordCursor.take(recordCursor.remaining, "Events payload"),
      ),
    };
    recordCursor.end("Events record");
    records.push(record);
  }
  cursor.end("Events batch");
  const value = { firstSequence, records };
  validateBatch(value);
  return value;
}

export function encodeEventsRecordEvent(
  value: YasEventsRecordEvent,
): Uint8Array {
  requireHandle(value.streamHandle, "Events stream");
  return new YasWriter()
    .u64(value.streamHandle)
    .u16(g.YAS_EVENTS_CODEC_V1)
    .u16(g.YAS_PACKED_CODEC_EVENTS_CODEC_V1_VERSION)
    .bytes(encodeEventsBatch(value.batch))
    .finish();
}

export function decodeEventsRecordEvent(
  bytes: Uint8Array,
): YasEventsRecordEvent {
  const cursor = new YasCursor(bytes);
  const streamHandle = cursor.u64("Events stream handle");
  if (
    cursor.u16("Events codec") !== g.YAS_EVENTS_CODEC_V1 ||
    cursor.u16("Events codec version") !==
      g.YAS_PACKED_CODEC_EVENTS_CODEC_V1_VERSION
  )
    throw new YasProtocolError("unknown Events packed codec");
  const value = {
    streamHandle,
    batch: decodeEventsBatch(cursor.take(cursor.remaining, "Events batch")),
  };
  cursor.end("Events RECORD");
  requireHandle(value.streamHandle, "Events stream");
  return value;
}

export function encodeEventsGap(value: YasEventsGap): Uint8Array {
  requireHandle(value.streamHandle, "Events stream");
  if (value.lost === 0n) throw new YasProtocolError("zero Events stream gap");
  return new YasWriter()
    .u64(value.streamHandle)
    .u64(value.lost)
    .u64(value.firstAvailableSequence)
    .finish();
}

export function decodeEventsGap(bytes: Uint8Array): YasEventsGap {
  const cursor = new YasCursor(bytes);
  const value = {
    streamHandle: cursor.u64("Events stream handle"),
    lost: cursor.u64("Events lost records"),
    firstAvailableSequence: cursor.u64("Events first available sequence"),
  };
  cursor.end("Events GAP");
  encodeEventsGap(value);
  return value;
}

export function encodeEventsStreamStopped(
  value: YasEventsStreamStopped,
): Uint8Array {
  requireHandle(value.streamHandle, "Events stream");
  if (utf8Length(value.detail) > g.YAS_EVENTS_MAX_RECORD_ERROR_BYTES)
    throw new YasProtocolError("Events stream detail exceeds its byte limit");
  rejectRequiredExtensions(value.extensions, "Events STREAM_STOPPED");
  return new YasWriter()
    .u64(value.streamHandle)
    .u16(value.status)
    .u16(0)
    .utf8U32(value.detail)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeEventsStreamStopped(
  bytes: Uint8Array,
): YasEventsStreamStopped {
  const cursor = new YasCursor(bytes);
  const streamHandle = cursor.u64("Events stream handle");
  const status = cursor.u16("Events stream status");
  if (cursor.u16("Events stream status reserved") !== 0)
    throw new YasProtocolError(
      "Events stream status reserved field is nonzero",
    );
  const value = {
    streamHandle,
    status,
    detail: cursor.utf8U32("Events stream detail"),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Events STREAM_STOPPED extensions",
    ),
  };
  cursor.end("Events STREAM_STOPPED");
  encodeEventsStreamStopped(value);
  return value;
}

interface StreamWaiter {
  resolve(value: YasEventsStreamItem | null): void;
  reject(error: unknown): void;
}

export interface YasEventsStreamQueuePolicy {
  maxItems: number;
  maxBytes: number;
}

interface QueuedEventsStreamItem {
  item: YasEventsStreamItem;
  bytes: number;
}

interface YasEventsStartOperation {
  payloadKey: string;
  pending: Promise<YasEventsStream> | null;
  stream: YasEventsStream | null;
  retainPayload: boolean;
}

const EVENTS_STREAM_MAX_QUEUED_ITEMS = 256;

export class YasEventsStream {
  private expectedSequence: bigint;
  private queue: QueuedEventsStreamItem[] = [];
  private queueBytes = 0;
  private waiters: StreamWaiter[] = [];
  private stopped = false;
  private failure: unknown | undefined;
  private recordListeners = new Set<(batch: YasEventBatch) => void>();
  private gapListeners = new Set<(gap: YasEventsGap) => void>();
  private stopListeners = new Set<(stopped: YasEventsStreamStopped) => void>();

  constructor(
    private readonly client: YasEventsClient,
    readonly started: YasEventsStreamStarted,
    private readonly queuePolicy: YasEventsStreamQueuePolicy = defaultEventsStreamQueuePolicy(
      started.maxBatchBytes,
    ),
  ) {
    this.expectedSequence = started.firstSequence;
  }

  get handle(): bigint {
    return this.started.streamHandle;
  }

  next(): Promise<YasEventsStreamItem | null> {
    const queued = this.queue.shift();
    if (queued) {
      this.queueBytes -= queued.bytes;
      return Promise.resolve(queued.item);
    }
    if (this.failure !== undefined) return Promise.reject(this.failure);
    if (this.stopped) return Promise.resolve(null);
    return new Promise((resolve, reject) =>
      this.waiters.push({ resolve, reject }),
    );
  }

  onRecords(listener: (batch: YasEventBatch) => void): () => void {
    this.recordListeners.add(listener);
    return () => this.recordListeners.delete(listener);
  }

  onGap(listener: (gap: YasEventsGap) => void): () => void {
    this.gapListeners.add(listener);
    return () => this.gapListeners.delete(listener);
  }

  onStopped(listener: (stopped: YasEventsStreamStopped) => void): () => void {
    this.stopListeners.add(listener);
    return () => this.stopListeners.delete(listener);
  }

  stop(
    operationId = randomOperationId(),
    extensions: readonly YasExtension[] = [],
  ): Promise<void> {
    return this.client.stopStream(this.handle, operationId, extensions);
  }

  receiveRecords(value: YasEventsRecordEvent): void {
    if (this.failure !== undefined) return;
    if (this.stopped)
      throw new YasProtocolError("Events RECORD followed STREAM_STOPPED");
    if (value.batch.firstSequence !== this.expectedSequence)
      throw new YasProtocolError(
        "Events RECORD sequence does not follow the stream cursor",
      );
    this.expectedSequence += BigInt(value.batch.records.length);
    for (const listener of this.recordListeners) {
      try {
        listener(value.batch);
      } catch {
        // One observer cannot fail Event dispatch for its siblings.
      }
    }
    this.push({ type: "records", batch: value.batch });
  }

  receiveGap(value: YasEventsGap): void {
    if (this.failure !== undefined) return;
    if (this.stopped)
      throw new YasProtocolError("Events GAP followed STREAM_STOPPED");
    if (value.firstAvailableSequence !== this.expectedSequence + value.lost)
      throw new YasProtocolError("Events GAP does not match the stream cursor");
    this.expectedSequence = value.firstAvailableSequence;
    for (const listener of this.gapListeners) {
      try {
        listener(value);
      } catch {
        // One observer cannot fail Event dispatch for its siblings.
      }
    }
    this.push({
      type: "gap",
      lost: value.lost,
      firstAvailableSequence: value.firstAvailableSequence,
    });
  }

  receiveStopped(value: YasEventsStreamStopped): void {
    if (this.failure !== undefined) {
      this.client.releaseStream(this.handle, this);
      return;
    }
    if (this.stopped) return;
    this.stopped = true;
    for (const listener of this.stopListeners) {
      try {
        listener(value);
      } catch {
        // One observer cannot fail Event dispatch for its siblings.
      }
    }
    this.push({ type: "stopped", status: value.status, detail: value.detail });
    this.client.releaseStream(this.handle, this);
  }

  invalidate(
    error = new YasDisconnectedError("YAS Events stream invalidated"),
  ): void {
    if (this.failure !== undefined) return;
    this.stopped = true;
    this.failure = error;
    for (const waiter of this.waiters.splice(0)) waiter.reject(error);
    this.queue = [];
    this.queueBytes = 0;
    this.recordListeners.clear();
    this.gapListeners.clear();
    this.stopListeners.clear();
  }

  private push(item: YasEventsStreamItem): void {
    const waiter = this.waiters.shift();
    if (waiter) {
      waiter.resolve(item);
      return;
    }
    const bytes = eventsStreamItemBytes(item);
    if (
      this.queue.length >= this.queuePolicy.maxItems ||
      bytes > this.queuePolicy.maxBytes - this.queueBytes
    ) {
      const shouldStopRemote = !this.stopped;
      const error = new YasProtocolError(
        "YAS Events stream consumer queue limit exceeded",
      );
      this.stopped = true;
      this.failure = error;
      this.queue = [];
      this.queueBytes = 0;
      for (const pending of this.waiters.splice(0)) pending.reject(error);
      this.recordListeners.clear();
      this.gapListeners.clear();
      this.stopListeners.clear();
      if (shouldStopRemote) {
        const retire = (
          this.client as YasEventsClient & {
            stopOverflowedStream?: (stream: YasEventsStream) => void;
          }
        ).stopOverflowedStream;
        if (retire) retire.call(this.client, this);
        else void this.client.stopStream(this.handle).catch(() => undefined);
      }
      return;
    }
    this.queue.push({ item, bytes });
    this.queueBytes += bytes;
  }
}

export class YasEventsClient {
  private readonly transfers;
  private readonly streams = new Map<bigint, YasEventsStream>();
  private readonly streamOperationKeys = new WeakMap<YasEventsStream, string>();
  private readonly startOperations = new Map<string, YasEventsStartOperation>();
  private readonly pendingStartOperations = new Map<
    string,
    YasEventsStartOperation
  >();
  private readonly activeDumpTransfers = new Set<YasTransfer>();
  private readonly pendingCancels = new Set<(error: unknown) => void>();
  private removeListeners: (() => void)[];
  private epoch = 0;
  private disposed = false;

  constructor(
    readonly connection: YasConnection,
    private readonly hashBytes: YasEventsHashBytes = defaultBlake3,
  ) {
    connection.family(g.YAS_FAMILY_EVENTS, g.YAS_EVENTS_VERSION);
    this.transfers = transfersFor(connection);
    this.removeListeners = [
      connection.onEvent(
        g.YAS_FAMILY_EVENTS,
        g.YAS_EVENTS_RECORD,
        ({ payload }) => {
          const value = decodeEventsRecordEvent(payload);
          this.requireStream(value.streamHandle).receiveRecords(value);
        },
      ),
      connection.onEvent(
        g.YAS_FAMILY_EVENTS,
        g.YAS_EVENTS_GAP,
        ({ payload }) => {
          const value = decodeEventsGap(payload);
          this.requireStream(value.streamHandle).receiveGap(value);
        },
      ),
      connection.onEvent(
        g.YAS_FAMILY_EVENTS,
        g.YAS_EVENTS_STREAM_STOPPED,
        ({ payload }) => {
          const value = decodeEventsStreamStopped(payload);
          this.requireStream(value.streamHandle).receiveStopped(value);
        },
      ),
      connection.onInvalidation(({ family }) => {
        if (family === undefined || family === g.YAS_FAMILY_EVENTS)
          this.invalidate();
      }),
    ];
  }

  getConfig(
    extensions: readonly YasExtension[] = [],
  ): Promise<YasEventsConfig> {
    this.assertOpen();
    return this.connection.requestDecoded(
      g.YAS_FAMILY_EVENTS,
      g.YAS_EVENTS_GET_CONFIG,
      encodeEventsGetConfig(extensions),
      decodeEventsConfig,
    );
  }

  setConfig(value: YasEventsSetConfig): Promise<YasEventsConfig> {
    this.assertOpen();
    return this.connection.requestDecoded(
      g.YAS_FAMILY_EVENTS,
      g.YAS_EVENTS_SET_CONFIG,
      encodeEventsSetConfig(value),
      decodeEventsConfig,
    );
  }

  dump(initialReceiveCredit = 4n * 1024n * 1024n): Promise<YasEventsDump> {
    this.assertOpen();
    const epoch = this.epoch;
    return this.runOwned(this.performDump(initialReceiveCredit, epoch));
  }

  private async performDump(
    initialReceiveCredit: bigint,
    epoch: number,
  ): Promise<YasEventsDump> {
    const lease = this.transfers.reserveReceiveCredit(
      initialReceiveCredit,
      4096n,
    );
    let accepted: YasTransfer | undefined;
    try {
      const result = await this.connection.requestDecoded(
        g.YAS_FAMILY_EVENTS,
        g.YAS_EVENTS_DUMP,
        encodeEventsDumpRequest({ initialReceiveCredit: lease.bytes }),
        (body) => {
          const decoded = decodeEventsDumpResult(body);
          accepted = this.transfers.acceptServerDescriptor(
            decoded.descriptor,
            lease,
          );
          this.activeDumpTransfers.add(accepted);
          return decoded;
        },
      );
      if (this.disposed || epoch !== this.epoch)
        throw new YasDisconnectedError(
          "Events DUMP completed after client disposal",
        );
      const bytes = await accepted!.collect(result.byteLength);
      const actual = await this.hashBytes(bytes);
      if (this.disposed || epoch !== this.epoch)
        throw new YasDisconnectedError(
          "Events DUMP completed after client disposal",
        );
      if (actual.length !== 32 || !equalBytes(actual, result.contentHash))
        throw new YasProtocolError("Events dump failed BLAKE3 verification");
      return { bytes, contentHash: result.contentHash };
    } catch (error) {
      if (!accepted) lease.release();
      else accepted.reset();
      throw error;
    } finally {
      if (accepted) this.activeDumpTransfers.delete(accepted);
    }
  }

  startStream(
    value: Partial<Omit<YasEventsStartStream, "operationId">> & {
      operationId?: Uint8Array;
    } = {},
  ): Promise<YasEventsStream> {
    this.assertOpen();
    const request: YasEventsStartStream = {
      operationId: value.operationId ?? randomOperationId(),
      history: value.history ?? false,
      startSequence: value.startSequence ?? 0n,
      maxBatchBytes: value.maxBatchBytes ?? 0,
      extensions: value.extensions,
    };
    const payload = encodeEventsStartStream(request);
    const operationKey = byteKey(request.operationId);
    const payloadKey = byteKey(payload);
    let operation =
      this.startOperations.get(operationKey) ??
      this.pendingStartOperations.get(operationKey);
    if (operation) {
      if (operation.payloadKey !== payloadKey)
        throw new YasProtocolError(
          "Events START_STREAM operation ID was reused with a different payload",
        );
      if (operation.pending) return operation.pending;
      if (operation.stream) {
        if (this.streams.get(operation.stream.handle) === operation.stream)
          return Promise.resolve(operation.stream);
        operation.stream = null;
      }
    } else {
      this.ensureStartReplaySlot(operationKey);
      operation = {
        payloadKey,
        pending: null,
        stream: null,
        retainPayload: false,
      };
      this.pendingStartOperations.set(operationKey, operation);
    }
    if (operation.retainPayload) this.ensureStartReplaySlot(operationKey);
    const epoch = this.epoch;
    let wireRequest: Promise<YasEventsStream>;
    try {
      wireRequest = this.connection
        .requestDecoded(
          g.YAS_FAMILY_EVENTS,
          g.YAS_EVENTS_START_STREAM,
          payload,
          (body) =>
            this.installStream(
              decodeEventsStreamStarted(body),
              operationKey,
              operation,
              epoch,
            ),
        )
        .then((result) =>
          result instanceof YasEventsStream
            ? result
            : this.installStream(
                result as YasEventsStreamStarted,
                operationKey,
                operation,
                epoch,
              ),
        );
    } catch (error) {
      if (!operation.stream && !operation.retainPayload)
        this.pendingStartOperations.delete(operationKey);
      throw error;
    }
    let pending!: Promise<YasEventsStream>;
    pending = this.runOwned(wireRequest)
      .then((stream) => {
        if (this.disposed || epoch !== this.epoch)
          throw new YasDisconnectedError(
            "Events START_STREAM completed after client disposal or family invalidation",
          );
        return stream;
      })
      .finally(() => {
        if (operation.pending !== pending) return;
        operation.pending = null;
        if (
          !operation.stream &&
          !operation.retainPayload &&
          this.pendingStartOperations.get(operationKey) === operation
        )
          this.pendingStartOperations.delete(operationKey);
      });
    operation.pending = pending;
    return pending;
  }

  async stopStream(
    streamHandle: bigint,
    operationId = randomOperationId(),
    extensions: readonly YasExtension[] = [],
  ): Promise<void> {
    const stream = this.streams.get(streamHandle);
    if (!stream) return;
    await this.requestStopStream(streamHandle, operationId, extensions);
    this.tombstoneStream(stream);
  }

  stopOverflowedStream(stream: YasEventsStream): void {
    if (this.streams.get(stream.handle) !== stream) return;
    this.tombstoneStream(stream);
    void this.requestStopStream(stream.handle).catch(() => undefined);
  }

  startRecording(
    value: YasEventsStartRecording,
  ): Promise<YasEventsRecordingInfo> {
    this.assertOpen();
    return this.connection.requestDecoded(
      g.YAS_FAMILY_EVENTS,
      g.YAS_EVENTS_START_RECORDING,
      encodeEventsStartRecording(value),
      decodeEventsRecordingInfo,
    );
  }

  stopRecording(
    recordingHandle: bigint,
    operationId = randomOperationId(),
    extensions: readonly YasExtension[] = [],
  ): Promise<YasEventsRecordingInfo> {
    this.assertOpen();
    return this.connection.requestDecoded(
      g.YAS_FAMILY_EVENTS,
      g.YAS_EVENTS_STOP_RECORDING,
      encodeEventsStopRecording(recordingHandle, operationId, extensions),
      decodeEventsRecordingInfo,
    );
  }

  listRecordings(
    extensions: readonly YasExtension[] = [],
  ): Promise<YasEventsRecordingInfo[]> {
    this.assertOpen();
    rejectRequiredExtensions(extensions, "Events LIST_RECORDINGS");
    return this.connection.requestDecoded(
      g.YAS_FAMILY_EVENTS,
      g.YAS_EVENTS_LIST_RECORDINGS,
      encodeExtensions(extensions),
      decodeEventsRecordingList,
    );
  }

  releaseStream(handle: bigint, stream: YasEventsStream): void {
    if (this.streams.get(handle) !== stream) return;
    this.streams.delete(handle);
    this.tombstoneStream(stream);
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.epoch++;
    const error = new YasDisconnectedError("YAS Events client was disposed");
    for (const cancel of [...this.pendingCancels]) cancel(error);
    this.pendingCancels.clear();
    for (const remove of this.removeListeners) remove();
    this.removeListeners = [];
    this.resetDumpTransfers();
    for (const stream of this.streams.values()) {
      this.tombstoneStream(stream);
      void this.requestStopStream(stream.handle).catch(() => undefined);
      stream.invalidate(
        new YasDisconnectedError("YAS Events client was disposed"),
      );
    }
    this.streams.clear();
    this.retirePendingStartOperations();
    this.startOperations.clear();
    this.pendingStartOperations.clear();
  }

  private installStream(
    started: YasEventsStreamStarted,
    operationKey: string,
    operation: YasEventsStartOperation,
    epoch: number,
  ): YasEventsStream {
    if (this.disposed || epoch !== this.epoch) {
      void this.stopStreamIfUnowned(started.streamHandle).catch(
        () => undefined,
      );
      throw new YasDisconnectedError(
        "Events START_STREAM completed after client disposal or family invalidation",
      );
    }
    if (this.streams.has(started.streamHandle))
      throw new YasProtocolError("Events stream handle was reused");
    if (operation.retainPayload && !operation.stream) {
      void this.stopStreamIfUnowned(started.streamHandle).catch(
        () => undefined,
      );
      throw new YasProtocolError(
        "Events START_STREAM replayed a retired stream instead of STALE",
      );
    }
    const stream = new YasEventsStream(
      this,
      started,
      eventsStreamQueuePolicy(this.connection, started.maxBatchBytes),
    );
    this.streams.set(started.streamHandle, stream);
    this.streamOperationKeys.set(stream, operationKey);
    operation.stream = stream;
    operation.retainPayload = true;
    if (!this.retainStartOperation(operationKey, operation)) {
      this.streams.delete(started.streamHandle);
      operation.stream = null;
      void this.stopStreamIfUnowned(started.streamHandle).catch(
        () => undefined,
      );
      throw new YasProtocolError(
        "Events START_STREAM replay ledger overflowed",
      );
    }
    return stream;
  }

  private tombstoneStream(stream: YasEventsStream): void {
    const operationKey = this.streamOperationKeys.get(stream);
    if (!operationKey) return;
    const operation = this.startOperations.get(operationKey);
    if (operation?.stream !== stream) return;
    operation.stream = null;
    operation.retainPayload = true;
  }

  private retirePendingStartOperations(): void {
    for (const operation of this.startOperations.values()) {
      if (!operation.pending) continue;
      operation.pending = null;
      operation.stream = null;
      operation.retainPayload = true;
    }
    for (const [operationKey, operation] of this.pendingStartOperations) {
      operation.pending = null;
      operation.stream = null;
      operation.retainPayload = true;
      this.retainStartOperation(operationKey, operation);
    }
    this.pendingStartOperations.clear();
  }

  private requireStream(handle: bigint): YasEventsStream {
    const stream = this.streams.get(handle);
    if (!stream)
      throw new YasProtocolError("Events Event names an unknown stream");
    return stream;
  }

  private invalidate(): void {
    this.epoch++;
    const error = new YasDisconnectedError("YAS Events client was invalidated");
    for (const cancel of [...this.pendingCancels]) cancel(error);
    this.pendingCancels.clear();
    this.resetDumpTransfers();
    for (const stream of this.streams.values()) {
      this.tombstoneStream(stream);
      stream.invalidate();
    }
    this.streams.clear();
    this.retirePendingStartOperations();
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

  private resetDumpTransfers(): void {
    for (const transfer of this.activeDumpTransfers) {
      try {
        transfer.reset();
      } catch {
        // The shared Transfer registry may already be invalidated.
      }
    }
    this.activeDumpTransfers.clear();
  }

  private requestStopStream(
    streamHandle: bigint,
    operationId = randomOperationId(),
    extensions: readonly YasExtension[] = [],
  ): Promise<Uint8Array> {
    return this.connection.request(
      g.YAS_FAMILY_EVENTS,
      g.YAS_EVENTS_STOP_STREAM,
      encodeEventsStopStream(streamHandle, operationId, extensions),
    );
  }

  private stopStreamIfUnowned(streamHandle: bigint): Promise<void> {
    if (this.streams.has(streamHandle)) return Promise.resolve();
    return this.requestStopStream(streamHandle).then(() => undefined);
  }

  private ensureStartReplaySlot(operationKey: string): void {
    let pinned = 0;
    for (const [key, operation] of this.startOperations) {
      if (key === operationKey) continue;
      if (operation.pending || operation.stream) pinned++;
    }
    for (const key of this.pendingStartOperations.keys())
      if (key !== operationKey) pinned++;
    if (pinned + 1 > this.startReplayLimit())
      throw new YasResultError(
        g.YAS_STATUS_RESOURCE_EXHAUSTED,
        new Uint8Array(0),
        "Events START_STREAM replay ledger is full",
      );
  }

  private retainStartOperation(
    operationKey: string,
    operation: YasEventsStartOperation,
  ): boolean {
    if (this.startOperations.get(operationKey) === operation) return true;
    const limit = this.startReplayLimit();
    let needed = this.startOperations.size - limit + 1;
    for (const [key, operation] of this.startOperations) {
      if (needed <= 0) break;
      if (!operation.pending && !operation.stream && operation.retainPayload) {
        this.startOperations.delete(key);
        needed--;
      }
    }
    if (needed > 0) return false;
    this.pendingStartOperations.delete(operationKey);
    this.startOperations.set(operationKey, operation);
    return true;
  }

  private startReplayLimit(): number {
    const extension = this.connection
      .family(g.YAS_FAMILY_EVENTS, g.YAS_EVENTS_VERSION)
      .limits.find(
        (candidate) =>
          candidate.tag === g.YAS_EVENTS_LIMIT_MAX_MUTATION_REPLAYS,
      );
    if (!extension)
      throw new YasProtocolError(
        "required Events mutation replay limit is absent",
      );
    const cursor = new YasCursor(extension.value);
    const value = cursor.u32("Events mutation replay limit");
    cursor.end("Events mutation replay limit");
    if (value === 0 || value > g.YAS_EVENTS_MAX_MUTATION_REPLAYS)
      throw new YasProtocolError("invalid Events mutation replay limit");
    return value;
  }

  private assertOpen(): void {
    if (this.disposed) throw new YasProtocolError("Events client is disposed");
  }
}

function eventsStreamQueuePolicy(
  connection: YasConnection,
  maxBatchBytes: number,
): YasEventsStreamQueuePolicy {
  return eventsStreamQueuePolicyForBudget(
    connection.options.receiveMaxBuffered ?? 16n * 1024n * 1024n,
    maxBatchBytes,
  );
}

function defaultEventsStreamQueuePolicy(
  maxBatchBytes: number,
): YasEventsStreamQueuePolicy {
  return eventsStreamQueuePolicyForBudget(16n * 1024n * 1024n, maxBatchBytes);
}

function eventsStreamQueuePolicyForBudget(
  receiveMaxBuffered: bigint,
  maxBatchBytes: number,
): YasEventsStreamQueuePolicy {
  const batchBytes = BigInt(Math.max(1, maxBatchBytes));
  const desired = batchBytes * BigInt(EVENTS_STREAM_MAX_QUEUED_ITEMS);
  // Conservatively partition the session's advertised aggregate receive
  // budget across the maximum number of concurrently negotiated v1 streams.
  // Immediate waiters still consume no queue budget.
  const perStream =
    receiveMaxBuffered / BigInt(g.YAS_EVENTS_MAX_STREAMS_PER_SESSION);
  const selected = perStream < desired ? perStream : desired;
  const maxBytes = Number(selected);
  return {
    maxItems: Math.max(
      1,
      Math.min(
        EVENTS_STREAM_MAX_QUEUED_ITEMS,
        Math.floor(maxBytes / Number(batchBytes)),
      ),
    ),
    maxBytes,
  };
}

function eventsStreamItemBytes(item: YasEventsStreamItem): number {
  if (item.type === "records") {
    let bytes = 12;
    for (const record of item.batch.records)
      bytes += 28 + record.payload.length;
    return bytes;
  }
  if (item.type === "gap") return 24;
  // Count the retained JS string conservatively as UTF-16 plus object/header
  // overhead. The item-count bound covers implementation-specific overhead.
  return 32 + item.detail.length * 2;
}

function encodeRecordingInfoTo(
  writer: YasWriter,
  value: YasEventsRecordingInfo,
): void {
  validateRecordingInfo(value);
  const flags =
    (value.history ? g.YAS_EVENTS_RECORDING_HISTORY : 0) |
    (value.append ? g.YAS_EVENTS_RECORDING_APPEND : 0);
  writer
    .u64(value.recordingHandle)
    .u8(value.state)
    .u8(0)
    .u16(flags)
    .u64(value.records)
    .u64(value.bytes)
    .u64(value.lost)
    .bytesU32(value.path)
    .utf8U32(value.error)
    .bytes(encodeExtensions(value.extensions));
}

function decodeRecordingInfoFrom(cursor: YasCursor): YasEventsRecordingInfo {
  const recordingHandle = cursor.u64("Events recording handle");
  const state = cursor.u8("Events recording state");
  if (cursor.u8("Events recording reserved") !== 0)
    throw new YasProtocolError("Events recording reserved byte is nonzero");
  const flags = cursor.u16("Events recording flags");
  if (flags & ~g.YAS_EVENTS_RECORDING_FLAGS)
    throw new YasProtocolError("invalid Events recording flags");
  const value = {
    recordingHandle,
    state,
    history: Boolean(flags & g.YAS_EVENTS_RECORDING_HISTORY),
    append: Boolean(flags & g.YAS_EVENTS_RECORDING_APPEND),
    records: cursor.u64("Events recording records"),
    bytes: cursor.u64("Events recording bytes"),
    lost: cursor.u64("Events recording lost records"),
    path: new Uint8Array(cursor.bytesU32("Events recording path")),
    error: cursor.utf8U32("Events recording error"),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Events recording extensions",
    ),
  };
  validateRecordingInfo(value);
  return value;
}

function validateConfig(value: YasEventsConfig): void {
  if (
    value.revision === 0n ||
    value.capacity < BigInt(g.YAS_EVENTS_MIN_RING_BYTES) ||
    value.capacity > BigInt(g.YAS_EVENTS_MAX_RING_BYTES) ||
    value.used > value.capacity ||
    value.recordCount > value.used ||
    value.nextSequence < value.recordCount
  )
    throw new YasProtocolError("invalid Events configuration counters");
  validateActivations(value.activations);
  rejectRequiredExtensions(value.extensions, "Events config");
}

function validateCapacity(value: bigint): void {
  if (
    value < BigInt(g.YAS_EVENTS_MIN_RING_BYTES) ||
    value > BigInt(g.YAS_EVENTS_MAX_RING_BYTES)
  )
    throw new YasProtocolError("invalid Events ring capacity");
}

function validateRecordingInfo(value: YasEventsRecordingInfo): void {
  requireHandle(value.recordingHandle, "Events recording");
  if (
    value.state < g.YAS_EVENTS_RECORDING_RUNNING ||
    value.state > g.YAS_EVENTS_RECORDING_FAILED
  )
    throw new YasProtocolError("invalid Events recording state");
  validateNativePath(value.path);
  if (utf8Length(value.error) > g.YAS_EVENTS_MAX_RECORD_ERROR_BYTES)
    throw new YasProtocolError("Events recording error exceeds its byte limit");
  if (value.state === g.YAS_EVENTS_RECORDING_RUNNING && value.error.length)
    throw new YasProtocolError("running Events recording has an error");
  rejectRequiredExtensions(value.extensions, "Events recording");
}

function validateBatch(value: YasEventBatch): void {
  if (
    value.records.length === 0 ||
    value.records.length > g.YAS_HARD_MAX_TYPED_RECORDS
  )
    throw new YasProtocolError("invalid Events record count");
  let bytes = 12;
  for (let index = 0; index < value.records.length; index++) {
    const record = value.records[index]!;
    if (record.sequence !== value.firstSequence + BigInt(index))
      throw new YasProtocolError("non-consecutive Events records");
    if (record.required && record.eventId > g.YAS_EVENTS_EVENT_SERVER_ERROR)
      throw new YasProtocolError("unknown required Events event ID");
    if (
      !Number.isInteger(record.eventId) ||
      record.eventId < 0 ||
      record.eventId > 0xffff_ffff
    )
      throw new YasProtocolError("invalid Events event ID");
    if (
      !Number.isInteger(record.eventFlags) ||
      record.eventFlags < 0 ||
      record.eventFlags > 0xffff
    )
      throw new YasProtocolError("invalid Events event flags");
    bytes += 28 + record.payload.length;
    if (bytes > YAS_MAX_DECODED_FRAME)
      throw new YasProtocolError(
        "Events batch exceeds the decoded-frame limit",
      );
  }
}

function validateDumpDescriptor(descriptor: YasTransferDescriptor): void {
  if (
    descriptor.mode !== YAS_TRANSFER_MODE_BYTE ||
    descriptor.direction !== YAS_TRANSFER_SENDER_TO_RECEIVER ||
    descriptor.receiverSendCredit !== 0n ||
    descriptor.maxItemBytes !== 0n ||
    descriptor.contentFamily !== g.YAS_FAMILY_EVENTS ||
    descriptor.contentKind !== g.YAS_EVENTS_DUMP_CONTENT_KIND ||
    descriptor.contentVersion !== g.YAS_EVENTS_VERSION ||
    descriptor.sensitiveContent !== true
  )
    throw new YasProtocolError("invalid Events dump Transfer descriptor");
}

function validateNativePath(path: Uint8Array): void {
  if (
    path.length === 0 ||
    path.length > g.YAS_EVENTS_MAX_RECORDING_PATH_BYTES ||
    path.includes(0)
  )
    throw new YasProtocolError("invalid Events recording path");
}

function validateActivations(values: readonly bigint[]): void {
  if (values.length !== g.YAS_EVENTS_ACTIVATION_WORDS)
    throw new YasProtocolError("invalid Events activation word count");
  for (const value of values)
    if (value < 0n || value > 0xffff_ffff_ffff_ffffn)
      throw new YasProtocolError("invalid Events activation word");
}

function encodeActivations(writer: YasWriter, values: readonly bigint[]): void {
  validateActivations(values);
  for (const value of values) writer.u64(value);
}

function decodeActivations(cursor: YasCursor): bigint[] {
  return Array.from({ length: g.YAS_EVENTS_ACTIVATION_WORDS }, () =>
    cursor.u64("Events activation word"),
  );
}

function requireOperationId(value: Uint8Array, name: string): void {
  if (value.length !== 16 || value.every((byte) => byte === 0))
    throw new YasProtocolError(`${name} operation ID is zero or malformed`);
}

function requireHandle(value: bigint, name: string): void {
  if (value === 0n) throw new YasProtocolError(`zero ${name} handle`);
}

function validateHash(value: Uint8Array, name: string): void {
  if (value.length !== 32)
    throw new YasProtocolError(`${name} content hash is not 32 bytes`);
}

function rejectRequiredExtensions(
  extensions: readonly YasExtension[],
  name: string,
): void {
  for (const extension of extensions)
    if (extension.required)
      throw new YasProtocolError(`${name} has an unknown required extension`);
}

function randomOperationId(): Uint8Array {
  const value = new Uint8Array(16);
  globalThis.crypto.getRandomValues(value);
  return value;
}

function byteKey(value: Uint8Array): string {
  let key = "";
  for (const byte of value) key += byte.toString(16).padStart(2, "0");
  return key;
}

function utf8Length(value: string): number {
  return new TextEncoder().encode(value).length;
}

async function defaultBlake3(bytes: Uint8Array): Promise<Uint8Array> {
  const { blake3_hash } = await import("@yas-run/browser");
  return blake3_hash(bytes);
}
