/** YAS Environment family codecs and client. */

import {
  YAS_ENV_DELIVERY_INLINE,
  YAS_ENV_DELIVERY_TRANSFER,
  YAS_ENV_GET,
  YAS_ENV_MAX_BATCH_BYTES,
  YAS_ENV_MAX_ENTRIES,
  YAS_ENV_MAX_INLINE_BYTES,
  YAS_ENV_MAX_KEY_BYTES,
  YAS_ENV_MAX_TOTAL_DATA_BYTES,
  YAS_ENV_MAX_VALUE_BYTES,
  YAS_ENV_SNAPSHOT_CONTENT_KIND,
  YAS_ENV_VERSION,
  YAS_FAMILY_ENV,
  YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION,
} from "./generated";
import type { YasConnection } from "./session";
import {
  YAS_TRANSFER_MODE_MESSAGE,
  YAS_TRANSFER_SENDER_TO_RECEIVER,
  decodeTransferDescriptor,
  encodeTransferDescriptor,
  transfersFor,
  type YasTransferDescriptor,
} from "./transfer";
import {
  YasCursor,
  YasProtocolError,
  YasWriter,
  decodeExtensions,
  encodeExtensions,
  type YasExtension,
} from "./wire";

export {
  YAS_ENV_DELIVERY_INLINE,
  YAS_ENV_DELIVERY_TRANSFER,
  YAS_ENV_GET,
  YAS_ENV_MAX_BATCH_BYTES,
  YAS_ENV_MAX_ENTRIES,
  YAS_ENV_MAX_INLINE_BYTES,
  YAS_ENV_MAX_KEY_BYTES,
  YAS_ENV_MAX_TOTAL_DATA_BYTES,
  YAS_ENV_MAX_VALUE_BYTES,
  YAS_ENV_SNAPSHOT_CONTENT_KIND,
  YAS_ENV_VERSION,
} from "./generated";

export interface YasEnvEntry {
  key: Uint8Array;
  value: Uint8Array;
}

export interface YasEnvSnapshot {
  entries: readonly YasEnvEntry[];
  totalDataBytes: bigint;
}

export interface YasEnvGet {
  initialReceiveCredit: bigint;
  extensions?: readonly YasExtension[];
}

export type YasEnvDelivery =
  | { type: "inline"; entries: readonly YasEnvEntry[] }
  | { type: "transfer"; descriptor: YasTransferDescriptor };

export interface YasEnvGetResult {
  entryCount: number;
  totalDataBytes: bigint;
  delivery: YasEnvDelivery;
  extensions: readonly YasExtension[];
}

export interface YasEnvSnapshotBatch {
  firstIndex: number;
  entries: readonly YasEnvEntry[];
}

export function encodeEnvGet(value: YasEnvGet): Uint8Array {
  rejectRequiredExtensions(value.extensions ?? [], new Set(), "Env GET");
  return new YasWriter()
    .u64(value.initialReceiveCredit)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeEnvGet(bytes: Uint8Array): Required<YasEnvGet> {
  const cursor = new YasCursor(bytes);
  const initialReceiveCredit = cursor.u64("Env initial receive credit");
  const extensions = decodeExtensions(cursor, new Set(), "Env GET extensions");
  cursor.end("Env GET");
  return { initialReceiveCredit, extensions };
}

export function encodeEnvEntry(entry: YasEnvEntry): Uint8Array {
  validateEntry(entry);
  return new YasWriter().bytesU16(entry.key).bytesU32(entry.value).finish();
}

export function decodeEnvEntry(bytes: Uint8Array): YasEnvEntry {
  const cursor = new YasCursor(bytes);
  const value = decodeEntry(cursor);
  cursor.end("Env entry");
  return value;
}

export function encodeEnvGetResult(value: YasEnvGetResult): Uint8Array {
  validateResult(value);
  const writer = new YasWriter();
  if (value.delivery.type === "inline") {
    writer
      .u8(YAS_ENV_DELIVERY_INLINE)
      .bytes(new Uint8Array(3))
      .u32(value.entryCount)
      .u64(value.totalDataBytes);
    for (const entry of value.delivery.entries)
      writer.bytes(encodeEnvEntry(entry));
  } else {
    writer
      .u8(YAS_ENV_DELIVERY_TRANSFER)
      .bytes(new Uint8Array(3))
      .u32(value.entryCount)
      .u64(value.totalDataBytes)
      .bytesU32(encodeTransferDescriptor(value.delivery.descriptor));
  }
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeEnvGetResult(bytes: Uint8Array): YasEnvGetResult {
  const cursor = new YasCursor(bytes);
  const delivery = cursor.u8("Env delivery");
  if (cursor.take(3, "Env result reserved").some((byte) => byte !== 0))
    throw new YasProtocolError("Env result reserved bytes are nonzero");
  const entryCount = cursor.u32("Env entry count");
  const totalDataBytes = cursor.u64("Env total data bytes");
  validateSummary(entryCount, totalDataBytes);

  let decodedDelivery: YasEnvDelivery;
  if (delivery === YAS_ENV_DELIVERY_INLINE) {
    const entries: YasEnvEntry[] = [];
    for (let index = 0; index < entryCount; index++)
      entries.push(decodeEntry(cursor));
    decodedDelivery = { type: "inline", entries };
  } else if (delivery === YAS_ENV_DELIVERY_TRANSFER) {
    const descriptorCursor = cursor.sub(
      cursor.u32("Env Transfer descriptor length"),
      "Env Transfer descriptor",
    );
    const descriptor = decodeTransferDescriptor(descriptorCursor);
    descriptorCursor.end("Env Transfer descriptor");
    decodedDelivery = { type: "transfer", descriptor };
  } else {
    throw new YasProtocolError("unknown Env result delivery");
  }
  const extensions = decodeExtensions(
    cursor,
    new Set(),
    "Env Result extensions",
  );
  cursor.end("Env GET Result");
  const value = {
    entryCount,
    totalDataBytes,
    delivery: decodedDelivery,
    extensions,
  };
  validateResult(value);
  return value;
}

export function encodeEnvSnapshotBatch(batch: YasEnvSnapshotBatch): Uint8Array {
  validateBatch(batch);
  const writer = new YasWriter()
    .u32(batch.firstIndex)
    .u16(batch.entries.length)
    .u16(0);
  for (const entry of batch.entries) writer.bytes(encodeEnvEntry(entry));
  return writer.finish();
}

export function decodeEnvSnapshotBatch(bytes: Uint8Array): YasEnvSnapshotBatch {
  if (bytes.length > YAS_ENV_MAX_BATCH_BYTES)
    throw new YasProtocolError("Env snapshot batch exceeds its byte limit");
  const cursor = new YasCursor(bytes);
  const firstIndex = cursor.u32("Env batch first index");
  const count = cursor.u16("Env batch entry count");
  if (cursor.u16("Env batch reserved") !== 0)
    throw new YasProtocolError("Env batch reserved field is nonzero");
  if (count === 0 || count > YAS_ENV_MAX_ENTRIES)
    throw new YasProtocolError("invalid Env batch entry count");
  const entries: YasEnvEntry[] = [];
  for (let index = 0; index < count; index++) entries.push(decodeEntry(cursor));
  cursor.end("Env snapshot batch");
  const value = { firstIndex, entries };
  validateBatch(value);
  return value;
}

export class YasEnvSnapshotAssembler {
  private readonly entries: YasEnvEntry[] = [];
  private totalDataBytes = 0n;

  constructor(
    private readonly expectedEntryCount: number,
    private readonly expectedTotalDataBytes: bigint,
  ) {
    validateSummary(expectedEntryCount, expectedTotalDataBytes);
    if (expectedEntryCount === 0)
      throw new YasProtocolError(
        "an empty environment must use inline delivery",
      );
    if (expectedTotalDataBytes === 0n)
      throw new YasProtocolError(
        "a transferred environment cannot have zero data bytes",
      );
  }

  push(batch: YasEnvSnapshotBatch): void {
    validateBatch(batch);
    if (batch.firstIndex !== this.entries.length)
      throw new YasProtocolError("non-contiguous Env snapshot batch index");
    if (this.entries.length + batch.entries.length > this.expectedEntryCount)
      throw new YasProtocolError("Env snapshot contains too many entries");
    const previous = this.entries[this.entries.length - 1];
    const first = batch.entries[0];
    if (previous && first && compareBytes(previous.key, first.key) >= 0)
      throw new YasProtocolError("Env keys are not strictly ascending");
    for (const entry of batch.entries) {
      this.totalDataBytes += BigInt(entry.key.length + entry.value.length);
      if (this.totalDataBytes > this.expectedTotalDataBytes)
        throw new YasProtocolError("Env snapshot exceeds its declared size");
      this.entries.push(copyEntry(entry));
    }
  }

  finish(): YasEnvSnapshot {
    if (
      this.entries.length !== this.expectedEntryCount ||
      this.totalDataBytes !== this.expectedTotalDataBytes
    )
      throw new YasProtocolError("incomplete Env snapshot");
    return { entries: this.entries, totalDataBytes: this.totalDataBytes };
  }
}

export class YasEnvClient {
  private readonly transfers;

  constructor(readonly connection: YasConnection) {
    connection.family(YAS_FAMILY_ENV, YAS_ENV_VERSION);
    this.transfers = transfersFor(connection);
  }

  async get(
    initialReceiveCredit = 5n * 1024n * 1024n,
  ): Promise<YasEnvSnapshot> {
    const lease = this.transfers.reserveReceiveCredit(
      initialReceiveCredit,
      64n * 1024n,
    );
    let transferAccepted = false;
    let leaseReleased = false;
    try {
      const result = await this.connection.requestDecoded(
        YAS_FAMILY_ENV,
        YAS_ENV_GET,
        encodeEnvGet({ initialReceiveCredit: lease.bytes }),
        (body) => {
          const decoded = decodeEnvGetResult(body);
          if (decoded.delivery.type === "inline") {
            lease.release();
            leaseReleased = true;
            return {
              inlineEntries: decoded.delivery.entries,
              totalDataBytes: decoded.totalDataBytes,
            };
          }
          validateEnvTransfer(decoded.delivery.descriptor);
          const transfer = this.transfers.acceptServerDescriptor(
            decoded.delivery.descriptor,
            lease,
          );
          transferAccepted = true;
          return {
            entryCount: decoded.entryCount,
            totalDataBytes: decoded.totalDataBytes,
            transfer,
          };
        },
      );
      if (result.inlineEntries !== undefined) {
        return {
          entries: result.inlineEntries.map(copyEntry),
          totalDataBytes: result.totalDataBytes,
        };
      }
      const assembler = new YasEnvSnapshotAssembler(
        result.entryCount,
        result.totalDataBytes,
      );
      while (true) {
        const message = await result.transfer.readMessage();
        if (message === null) break;
        assembler.push(decodeEnvSnapshotBatch(message));
      }
      return assembler.finish();
    } catch (error) {
      if (!transferAccepted && !leaseReleased) lease.release();
      throw error;
    }
  }
}

function decodeEntry(cursor: YasCursor): YasEnvEntry {
  const value = {
    key: new Uint8Array(cursor.bytesU16("environment key")),
    value: new Uint8Array(cursor.bytesU32("environment value")),
  };
  validateEntry(value);
  return value;
}

function validateEntry(entry: YasEnvEntry): void {
  if (entry.key.length === 0 || entry.key.length > YAS_ENV_MAX_KEY_BYTES)
    throw new YasProtocolError("invalid environment key length");
  if (entry.value.length > YAS_ENV_MAX_VALUE_BYTES)
    throw new YasProtocolError("environment value exceeds its byte limit");
  if (entry.key.includes(0) || entry.key.includes(0x3d))
    throw new YasProtocolError("environment key contains NUL or equals");
  if (entry.value.includes(0))
    throw new YasProtocolError("environment value contains NUL");
}

function validateResult(value: YasEnvGetResult): void {
  validateSummary(value.entryCount, value.totalDataBytes);
  rejectRequiredExtensions(value.extensions, new Set(), "Env Result");
  if (value.delivery.type === "inline") {
    const summary = summarizeEntries(value.delivery.entries, true);
    if (
      summary.entryCount !== value.entryCount ||
      summary.totalDataBytes !== value.totalDataBytes
    )
      throw new YasProtocolError("Env Result summary does not match entries");
  } else {
    if (value.entryCount === 0 || value.totalDataBytes === 0n)
      throw new YasProtocolError(
        "an empty environment must use inline delivery",
      );
    validateEnvTransfer(value.delivery.descriptor);
  }
}

function validateBatch(batch: YasEnvSnapshotBatch): void {
  if (
    batch.entries.length === 0 ||
    batch.entries.length > YAS_ENV_MAX_ENTRIES ||
    batch.entries.length > 0xffff ||
    batch.firstIndex + batch.entries.length > 0x1_0000_0000
  )
    throw new YasProtocolError("invalid Env snapshot batch count");
  const summary = summarizeEntries(batch.entries, false);
  if (8 + summary.encodedEntryBytes > YAS_ENV_MAX_BATCH_BYTES)
    throw new YasProtocolError("Env snapshot batch exceeds its byte limit");
}

function summarizeEntries(
  entries: readonly YasEnvEntry[],
  inline: boolean,
): {
  entryCount: number;
  totalDataBytes: bigint;
  encodedEntryBytes: number;
} {
  if (entries.length > YAS_ENV_MAX_ENTRIES)
    throw new YasProtocolError("too many environment entries");
  let totalDataBytes = 0n;
  let encodedEntryBytes = 0;
  let previous: Uint8Array | undefined;
  for (const entry of entries) {
    validateEntry(entry);
    if (previous && compareBytes(previous, entry.key) >= 0)
      throw new YasProtocolError("Env keys are not strictly ascending");
    previous = entry.key;
    totalDataBytes += BigInt(entry.key.length + entry.value.length);
    encodedEntryBytes += 6 + entry.key.length + entry.value.length;
    if (totalDataBytes > BigInt(YAS_ENV_MAX_TOTAL_DATA_BYTES))
      throw new YasProtocolError("environment data exceeds its total limit");
  }
  if (inline && encodedEntryBytes > YAS_ENV_MAX_INLINE_BYTES)
    throw new YasProtocolError("inline environment exceeds its byte limit");
  return { entryCount: entries.length, totalDataBytes, encodedEntryBytes };
}

function validateSummary(entryCount: number, totalDataBytes: bigint): void {
  if (
    !Number.isInteger(entryCount) ||
    entryCount < 0 ||
    entryCount > YAS_ENV_MAX_ENTRIES
  )
    throw new YasProtocolError("invalid environment entry count");
  if (
    totalDataBytes < 0n ||
    totalDataBytes > BigInt(YAS_ENV_MAX_TOTAL_DATA_BYTES)
  )
    throw new YasProtocolError("invalid environment total data bytes");
}

function validateEnvTransfer(descriptor: YasTransferDescriptor): void {
  const sensitive = descriptor.extensions.some(
    (extension) =>
      extension.tag === YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION &&
      extension.required === true &&
      extension.value.length === 0,
  );
  if (
    descriptor.mode !== YAS_TRANSFER_MODE_MESSAGE ||
    descriptor.direction !== YAS_TRANSFER_SENDER_TO_RECEIVER ||
    descriptor.contentFamily !== YAS_FAMILY_ENV ||
    descriptor.contentKind !== YAS_ENV_SNAPSHOT_CONTENT_KIND ||
    descriptor.contentVersion !== YAS_ENV_VERSION ||
    descriptor.maxItemBytes === 0n ||
    descriptor.maxItemBytes > BigInt(YAS_ENV_MAX_BATCH_BYTES) ||
    !sensitive
  )
    throw new YasProtocolError(
      "Env Result returned the wrong Transfer content type",
    );
}

function compareBytes(left: Uint8Array, right: Uint8Array): number {
  const length = Math.min(left.length, right.length);
  for (let index = 0; index < length; index++) {
    const difference = left[index]! - right[index]!;
    if (difference !== 0) return difference;
  }
  return left.length - right.length;
}

function copyEntry(entry: YasEnvEntry): YasEnvEntry {
  return {
    key: new Uint8Array(entry.key),
    value: new Uint8Array(entry.value),
  };
}

function rejectRequiredExtensions(
  extensions: readonly YasExtension[],
  known: ReadonlySet<number>,
  context: string,
): void {
  for (const extension of extensions)
    if (extension.required && !known.has(extension.tag))
      throw new YasProtocolError(
        `${context} contains unknown required extension ${extension.tag}`,
      );
}
