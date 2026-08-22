/** Low-level YAS v1 framing and scalar codecs. */

import {
  YAS_CLASS_EVENT,
  YAS_CLASS_REQUEST,
  YAS_CLASS_RESULT,
  YAS_EVENT_HEADER_BYTES,
  YAS_HARD_MAX_BUFFERED,
  YAS_HARD_MAX_BULK_CHUNK,
  YAS_HARD_MAX_DATAGRAM,
  YAS_HARD_MAX_DECODED_FRAME,
  YAS_HARD_MAX_WIRE_FRAME,
  YAS_META_COMPRESSED,
  YAS_META_RESERVED,
  YAS_META_SENSITIVE,
  YAS_OPERATION_POLICIES,
  YAS_PREFACE_HEX,
  YAS_PRE_HELLO_MAX_FRAME,
  YAS_STATUS_OK,
  YAS_STREAM_LENGTH_BYTES,
} from "./generated";

export {
  YAS_CLASS_EVENT,
  YAS_CLASS_REQUEST,
  YAS_CLASS_RESULT,
  YAS_META_COMPRESSED,
  YAS_META_SENSITIVE,
  YAS_STATUS_OK,
  YAS_STATUS_INVALID,
  YAS_STATUS_UNSUPPORTED,
  YAS_STATUS_NOT_FOUND,
  YAS_STATUS_CONFLICT,
  YAS_STATUS_BUSY,
  YAS_STATUS_UNAVAILABLE,
  YAS_STATUS_RESOURCE_EXHAUSTED,
  YAS_STATUS_RATE_LIMITED,
  YAS_STATUS_TIMEOUT,
  YAS_STATUS_CANCELLED,
  YAS_STATUS_STALE,
  YAS_STATUS_IO,
  YAS_STATUS_INTERNAL,
} from "./generated";

export const YAS_PREFACE = Uint8Array.from(
  YAS_PREFACE_HEX.match(/../g)!.map((byte) => Number.parseInt(byte, 16)),
);
export const YAS_META_KNOWN = ~YAS_META_RESERVED & 0xff;
export const YAS_MAX_PRE_HELLO_FRAME = YAS_PRE_HELLO_MAX_FRAME;
export const YAS_MAX_WIRE_FRAME = YAS_HARD_MAX_WIRE_FRAME;
export const YAS_MAX_DECODED_FRAME = YAS_HARD_MAX_DECODED_FRAME;
export const YAS_MAX_BULK_CHUNK = YAS_HARD_MAX_BULK_CHUNK;
export const YAS_MAX_DATAGRAM = YAS_HARD_MAX_DATAGRAM;
export const YAS_MAX_BUFFERED = BigInt(YAS_HARD_MAX_BUFFERED);

const utf8Encoder = new TextEncoder();
const utf8Decoder = new TextDecoder("utf-8", { fatal: true });

export class YasProtocolError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "YasProtocolError";
  }
}

export class YasDisconnectedError extends Error {
  constructor(message = "YAS link disconnected") {
    super(message);
    this.name = "YasDisconnectedError";
  }
}

export class YasResultError extends Error {
  constructor(
    readonly status: number,
    readonly detail: Uint8Array,
    message = `YAS request failed with status ${status}`,
  ) {
    super(message);
    this.name = "YasResultError";
  }
}

export class YasWriter {
  private chunks: Uint8Array[] = [];
  private size = 0;

  get length(): number {
    return this.size;
  }

  u8(value: number): this {
    return this.fixed(1, (view) => view.setUint8(0, checkedInt(value, 0xff)));
  }

  u16(value: number): this {
    return this.fixed(2, (view) =>
      view.setUint16(0, checkedInt(value, 0xffff), true),
    );
  }

  i16(value: number): this {
    if (!Number.isInteger(value) || value < -0x8000 || value > 0x7fff)
      throw new RangeError("value is not an i16");
    return this.fixed(2, (view) => view.setInt16(0, value, true));
  }

  u32(value: number): this {
    return this.fixed(4, (view) =>
      view.setUint32(0, checkedInt(value, 0xffff_ffff), true),
    );
  }

  i32(value: number): this {
    if (!Number.isInteger(value) || value < -0x8000_0000 || value > 0x7fff_ffff)
      throw new RangeError("value is not an i32");
    return this.fixed(4, (view) => view.setInt32(0, value, true));
  }

  u64(value: bigint): this {
    if (value < 0n || value > 0xffff_ffff_ffff_ffffn)
      throw new RangeError("value is not a u64");
    return this.fixed(8, (view) => view.setBigUint64(0, value, true));
  }

  i64(value: bigint): this {
    if (value < -0x8000_0000_0000_0000n || value > 0x7fff_ffff_ffff_ffffn)
      throw new RangeError("value is not an i64");
    return this.fixed(8, (view) => view.setBigInt64(0, value, true));
  }

  bytes(value: Uint8Array): this {
    const copy = new Uint8Array(value);
    this.chunks.push(copy);
    this.size += copy.length;
    return this;
  }

  utf8U16(value: string): this {
    const bytes = utf8Encoder.encode(value);
    if (bytes.length > 0xffff) throw new RangeError("UTF-8 string exceeds u16");
    return this.u16(bytes.length).bytes(bytes);
  }

  utf8U32(value: string): this {
    const bytes = utf8Encoder.encode(value);
    return this.u32(bytes.length).bytes(bytes);
  }

  bytesU32(value: Uint8Array): this {
    return this.u32(value.length).bytes(value);
  }

  bytesU16(value: Uint8Array): this {
    if (value.length > 0xffff) throw new RangeError("byte string exceeds u16");
    return this.u16(value.length).bytes(value);
  }

  finish(): Uint8Array {
    const output = new Uint8Array(this.size);
    let offset = 0;
    for (const chunk of this.chunks) {
      output.set(chunk, offset);
      offset += chunk.length;
    }
    return output;
  }

  private fixed(length: number, set: (view: DataView) => void): this {
    const bytes = new Uint8Array(length);
    set(new DataView(bytes.buffer));
    this.chunks.push(bytes);
    this.size += length;
    return this;
  }
}

function checkedInt(value: number, max: number): number {
  if (!Number.isInteger(value) || value < 0 || value > max)
    throw new RangeError(`value is not a u${Math.log2(max + 1)}`);
  return value;
}

export class YasCursor {
  offset = 0;

  constructor(readonly bytes: Uint8Array) {}

  get remaining(): number {
    return this.bytes.length - this.offset;
  }

  take(length: number, field = "bytes"): Uint8Array {
    if (!Number.isSafeInteger(length) || length < 0 || length > this.remaining)
      throw new YasProtocolError(`truncated or invalid ${field}`);
    const value = this.bytes.subarray(this.offset, this.offset + length);
    this.offset += length;
    return value;
  }

  u8(field = "u8"): number {
    return this.take(1, field)[0]!;
  }

  u16(field = "u16"): number {
    const value = this.take(2, field);
    return new DataView(value.buffer, value.byteOffset, 2).getUint16(0, true);
  }

  i16(field = "i16"): number {
    const value = this.take(2, field);
    return new DataView(value.buffer, value.byteOffset, 2).getInt16(0, true);
  }

  u32(field = "u32"): number {
    const value = this.take(4, field);
    return new DataView(value.buffer, value.byteOffset, 4).getUint32(0, true);
  }

  i32(field = "i32"): number {
    const value = this.take(4, field);
    return new DataView(value.buffer, value.byteOffset, 4).getInt32(0, true);
  }

  u64(field = "u64"): bigint {
    const value = this.take(8, field);
    return new DataView(value.buffer, value.byteOffset, 8).getBigUint64(
      0,
      true,
    );
  }

  i64(field = "i64"): bigint {
    const value = this.take(8, field);
    return new DataView(value.buffer, value.byteOffset, 8).getBigInt64(0, true);
  }

  utf8(length: number, field = "UTF-8 string"): string {
    try {
      return utf8Decoder.decode(this.take(length, field));
    } catch {
      throw new YasProtocolError(`invalid ${field}`);
    }
  }

  utf8U16(field = "UTF-8 string"): string {
    return this.utf8(this.u16(`${field} length`), field);
  }

  utf8U32(field = "UTF-8 string"): string {
    return this.utf8(this.u32(`${field} length`), field);
  }

  bytesU32(field = "byte string"): Uint8Array {
    return this.take(this.u32(`${field} length`), field);
  }

  bytesU16(field = "byte string"): Uint8Array {
    return this.take(this.u16(`${field} length`), field);
  }

  sub(length: number, field = "record"): YasCursor {
    return new YasCursor(this.take(length, field));
  }

  end(context = "payload"): void {
    if (this.remaining !== 0)
      throw new YasProtocolError(`unconsumed bytes in ${context}`);
  }
}

export interface YasExtension {
  tag: number;
  required?: boolean;
  value: Uint8Array;
}

export function encodeExtensions(
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  const body = new YasWriter();
  let previous = -1;
  for (const extension of extensions) {
    if (extension.tag <= previous)
      throw new YasProtocolError("extension tags must be unique and ascending");
    previous = extension.tag;
    body
      .u16(extension.tag)
      .u16(extension.required ? 1 : 0)
      .bytesU32(extension.value);
  }
  return new YasWriter().bytesU32(body.finish()).finish();
}

/** Decode an extension tail, including its leading extensions_len. */
export function decodeExtensions(
  cursor: YasCursor,
  knownTags?: ReadonlySet<number>,
  context = "extensions",
): YasExtension[] {
  const extensionsCursor = cursor.sub(cursor.u32(`${context} length`), context);
  const extensions: YasExtension[] = [];
  let previous = -1;
  while (extensionsCursor.remaining !== 0) {
    const tag = extensionsCursor.u16("extension tag");
    const flags = extensionsCursor.u16("extension flags");
    if (flags & ~1)
      throw new YasProtocolError("reserved extension flags are nonzero");
    if (tag <= previous)
      throw new YasProtocolError("duplicate or out-of-order extension tag");
    previous = tag;
    const value = extensionsCursor.bytesU32("extension value");
    if ((flags & 1) !== 0 && (!knownTags || !knownTags.has(tag)))
      throw new YasProtocolError(`unknown required extension ${tag}`);
    extensions.push({ tag, required: (flags & 1) !== 0, value });
  }
  return extensions;
}

export interface YasTypedRecord {
  kind: number;
  flags: number;
  body: Uint8Array;
}

export function encodeTypedRecord(record: YasTypedRecord): Uint8Array {
  if (record.flags & ~1)
    throw new YasProtocolError("reserved record flags are nonzero");
  return new YasWriter()
    .u32(4 + record.body.length)
    .u16(record.kind)
    .u16(record.flags)
    .bytes(record.body)
    .finish();
}

export function decodeTypedRecord(cursor: YasCursor): YasTypedRecord {
  const record = cursor.sub(cursor.u32("record length"), "record");
  if (record.remaining < 4)
    throw new YasProtocolError("record is shorter than header");
  const kind = record.u16("record kind");
  const flags = record.u16("record flags");
  if (flags & ~1)
    throw new YasProtocolError("reserved record flags are nonzero");
  return { kind, flags, body: record.take(record.remaining) };
}

export interface YasFrame {
  family: number;
  kind: number;
  class: number;
  requestId?: number;
  compressed: boolean;
  sensitive: boolean;
  payload: Uint8Array;
}

export interface YasFrameOperationPolicy {
  sensitive: "allowed" | "required" | "forbidden";
  compression: "allowed" | "required" | "forbidden";
  /** Generated transport-datagram predicate; reliable delivery remains legal. */
  datagram: number;
}

const POLICY_NAMES = ["allowed", "required", "forbidden"] as const;

/** Canonical metadata policy generated from the YAS TOML registry. */
export function yasOperationPolicy(
  family: number,
  frameClass: number,
  kind: number,
): YasFrameOperationPolicy | undefined {
  const policy = YAS_OPERATION_POLICIES[`${family}/${frameClass}/${kind}`];
  return policy
    ? {
        sensitive: POLICY_NAMES[policy[0]],
        compression: POLICY_NAMES[policy[1]],
        datagram: policy[2],
      }
    : undefined;
}

function validateYasOperationPolicy(
  family: number,
  frameClass: number,
  kind: number,
  compressed: boolean,
  sensitive: boolean,
): void {
  const policy = yasOperationPolicy(family, frameClass, kind);
  if (!policy) return;
  if (policy.compression === "required" && !compressed)
    throw new YasProtocolError("missing required YAS frame compression");
  if (policy.compression === "forbidden" && compressed)
    throw new YasProtocolError(
      "compression is forbidden for this YAS operation",
    );
  if (policy.sensitive === "required" && !sensitive)
    throw new YasProtocolError("missing required YAS SENSITIVE flag");
  if (policy.sensitive === "forbidden" && sensitive)
    throw new YasProtocolError(
      "YAS SENSITIVE flag is forbidden for this operation",
    );
}

export function encodeYasFrame(
  frame: Omit<YasFrame, "compressed" | "sensitive"> & {
    compressed?: boolean;
    sensitive?: boolean;
  },
): Uint8Array {
  if (frame.class < 0 || frame.class > 2)
    throw new YasProtocolError("invalid YAS frame class");
  const compressed = frame.compressed ?? false;
  const sensitive =
    frame.sensitive ??
    yasOperationPolicy(frame.family, frame.class, frame.kind)?.sensitive ===
      "required";
  validateYasOperationPolicy(
    frame.family,
    frame.class,
    frame.kind,
    compressed,
    sensitive,
  );
  if (compressed)
    throw new YasProtocolError(
      "YAS LZ4 compression is not implemented by this client",
    );
  const correlated = frame.class !== YAS_CLASS_EVENT;
  if (correlated && (!frame.requestId || frame.requestId > 0xffff_ffff))
    throw new YasProtocolError(
      "correlated frame requires a nonzero u32 request ID",
    );
  const writer = new YasWriter()
    .u16(frame.family)
    .u16(frame.kind)
    .u8(frame.class | (sensitive ? YAS_META_SENSITIVE : 0));
  if (correlated) writer.u32(frame.requestId!);
  return writer.bytes(frame.payload).finish();
}

export function decodeYasFrame(
  bytes: Uint8Array,
  maxFrame: number = YAS_MAX_WIRE_FRAME,
): YasFrame {
  if (bytes.length < 5 || bytes.length > maxFrame)
    throw new YasProtocolError("YAS frame length is outside the receive limit");
  const cursor = new YasCursor(bytes);
  const family = cursor.u16("family");
  const kind = cursor.u16("kind");
  const meta = cursor.u8("meta");
  if (meta & ~YAS_META_KNOWN)
    throw new YasProtocolError("reserved YAS meta bits are nonzero");
  const frameClass = meta & 3;
  if (frameClass === 3) throw new YasProtocolError("reserved YAS frame class");
  const compressed = (meta & YAS_META_COMPRESSED) !== 0;
  const sensitive = (meta & YAS_META_SENSITIVE) !== 0;
  validateYasOperationPolicy(family, frameClass, kind, compressed, sensitive);
  if (compressed)
    throw new YasProtocolError("received an unnegotiated compressed YAS frame");
  const requestId =
    frameClass === YAS_CLASS_EVENT ? undefined : cursor.u32("request ID");
  if (requestId === 0) throw new YasProtocolError("request ID zero is invalid");
  return {
    family,
    kind,
    class: frameClass,
    requestId,
    compressed,
    sensitive,
    payload: cursor.take(cursor.remaining),
  };
}

export function frameForByteStream(frame: Uint8Array): Uint8Array {
  return new YasWriter().u32(frame.length).bytes(frame).finish();
}

/** Incremental decoder for nested YAS byte streams. */
export class YasStreamFrameDecoder {
  private pending = new Uint8Array(0);

  constructor(private maxFrame: number = YAS_MAX_PRE_HELLO_FRAME) {}

  setMaxFrame(maxFrame: number): void {
    if (maxFrame < YAS_EVENT_HEADER_BYTES || maxFrame > YAS_MAX_WIRE_FRAME)
      throw new YasProtocolError("invalid stream frame limit");
    this.maxFrame = maxFrame;
  }

  push(chunk: Uint8Array): Uint8Array[] {
    if (chunk.length === 0) return [];
    const bytes = new Uint8Array(this.pending.length + chunk.length);
    bytes.set(this.pending);
    bytes.set(chunk, this.pending.length);
    const frames: Uint8Array[] = [];
    let offset = 0;
    while (bytes.length - offset >= YAS_STREAM_LENGTH_BYTES) {
      const length = new DataView(
        bytes.buffer,
        bytes.byteOffset + offset,
        YAS_STREAM_LENGTH_BYTES,
      ).getUint32(0, true);
      if (length < YAS_EVENT_HEADER_BYTES || length > this.maxFrame)
        throw new YasProtocolError("invalid length-prefixed YAS frame length");
      if (bytes.length - offset - YAS_STREAM_LENGTH_BYTES < length) break;
      frames.push(
        bytes.slice(
          offset + YAS_STREAM_LENGTH_BYTES,
          offset + YAS_STREAM_LENGTH_BYTES + length,
        ),
      );
      offset += YAS_STREAM_LENGTH_BYTES + length;
    }
    this.pending = bytes.slice(offset);
    if (this.pending.length > this.maxFrame + YAS_STREAM_LENGTH_BYTES)
      throw new YasProtocolError(
        "YAS stream buffering exceeded its frame limit",
      );
    return frames;
  }

  reset(): void {
    this.pending = new Uint8Array(0);
  }
}

export interface YasResultPrefix {
  status: number;
  detail: Uint8Array;
  body: Uint8Array;
}

export function encodeResultPayload(
  status: number,
  body: Uint8Array = new Uint8Array(0),
  detail: Uint8Array = new Uint8Array(0),
): Uint8Array {
  return new YasWriter()
    .u16(status)
    .u16(0)
    .bytesU32(detail)
    .bytes(body)
    .finish();
}

export function decodeResultPayload(payload: Uint8Array): YasResultPrefix {
  const cursor = new YasCursor(payload);
  const status = cursor.u16("result status");
  const flags = cursor.u16("result flags");
  if (flags !== 0)
    throw new YasProtocolError("reserved result flags are nonzero");
  const detail = cursor.bytesU32("result detail");
  // Detail is an extension body, not an extension tail with another length.
  validateExtensionBody(detail, "result detail");
  const body = cursor.take(cursor.remaining);
  if (status !== YAS_STATUS_OK && body.length !== 0)
    throw new YasProtocolError("failed YAS Result contains an operation body");
  return { status, detail, body };
}

export function validateExtensionBody(
  bytes: Uint8Array,
  context: string,
): void {
  const cursor = new YasCursor(bytes);
  let previous = -1;
  while (cursor.remaining !== 0) {
    const tag = cursor.u16(`${context} tag`);
    const flags = cursor.u16(`${context} flags`);
    if (flags & ~1)
      throw new YasProtocolError(`reserved ${context} flags are nonzero`);
    if (tag <= previous)
      throw new YasProtocolError(`duplicate or out-of-order ${context} tag`);
    previous = tag;
    cursor.take(cursor.u32(`${context} value length`), `${context} value`);
  }
}

export function equalBytes(left: Uint8Array, right: Uint8Array): boolean {
  if (left.length !== right.length) return false;
  let different = 0;
  for (let i = 0; i < left.length; i++) different |= left[i]! ^ right[i]!;
  return different === 0;
}
