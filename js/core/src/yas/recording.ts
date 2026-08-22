import {
  YAS_HARD_MAX_WIRE_FRAME,
  YAS_TERMINAL_FRAME_KEYFRAME,
  YAS_TERMINAL_GRID_CODEC_V1,
} from "./generated";
import { decodeTerminalFrame, type YasTerminalFrameEvent } from "./terminal";
import { YasCursor, YasProtocolError } from "./wire";

const YASREC1_MAGIC = new Uint8Array([
  0x59, 0x41, 0x53, 0x52, 0x45, 0x43, 0x31, 0x0a,
]);
const YASREC1_HEADER_BYTES = 36;
const YASREC1_TICKS_PER_SECOND = 1_000_000n;
const DEFAULT_MAX_RECORDING_BYTES = 256 * 1024 * 1024;
const DEFAULT_MAX_RECORDING_FRAMES = 1_000_000;

export interface YasTerminalRecordingHeader {
  gridCodec: number;
  terminalHandle: bigint;
  generation: number;
  rows: number;
  cols: number;
  viewId: number;
  firstSequence: number;
  ticksPerSecond: bigint;
}

export interface YasTerminalRecordingFrame {
  timestampTicks: bigint;
  frame: YasTerminalFrameEvent;
}

export interface YasTerminalRecording {
  header: YasTerminalRecordingHeader;
  frames: readonly YasTerminalRecordingFrame[];
}

export interface DecodeYasTerminalRecordingOptions {
  maxBytes?: number;
  maxFrames?: number;
  maxFrameBytes?: number;
}

/** Decode the native `YASREC1` TerminalFrame recording format. */
export function decodeYasTerminalRecording(
  input: ArrayBuffer | Uint8Array,
  options: DecodeYasTerminalRecordingOptions = {},
): YasTerminalRecording {
  const bytes = input instanceof Uint8Array ? input : new Uint8Array(input);
  const maxBytes = boundedOption(
    options.maxBytes,
    DEFAULT_MAX_RECORDING_BYTES,
    "recording byte limit",
  );
  const maxFrames = boundedOption(
    options.maxFrames,
    DEFAULT_MAX_RECORDING_FRAMES,
    "recording frame limit",
  );
  const maxFrameBytes = boundedOption(
    options.maxFrameBytes,
    YAS_HARD_MAX_WIRE_FRAME,
    "recording frame-byte limit",
  );
  if (bytes.length > maxBytes)
    throw new YasProtocolError("YASREC1 recording exceeds its byte limit");
  if (!startsWith(bytes, YASREC1_MAGIC))
    throw new YasProtocolError("invalid YASREC1 magic");

  const cursor = new YasCursor(bytes.subarray(YASREC1_MAGIC.length));
  if (cursor.u32("YASREC1 header length") !== YASREC1_HEADER_BYTES)
    throw new YasProtocolError("unsupported YASREC1 header length");
  if (cursor.u16("YASREC1 header flags") !== 0)
    throw new YasProtocolError("unsupported YASREC1 header flags");
  const header: YasTerminalRecordingHeader = {
    gridCodec: cursor.u16("YASREC1 grid codec"),
    terminalHandle: cursor.u64("YASREC1 Terminal handle"),
    generation: cursor.u32("YASREC1 Terminal generation"),
    rows: cursor.u16("YASREC1 initial rows"),
    cols: cursor.u16("YASREC1 initial columns"),
    viewId: cursor.u32("YASREC1 view ID"),
    firstSequence: cursor.u32("YASREC1 first sequence"),
    ticksPerSecond: cursor.u64("YASREC1 timestamp timebase"),
  };
  if (header.gridCodec !== YAS_TERMINAL_GRID_CODEC_V1)
    throw new YasProtocolError("unsupported YASREC1 Terminal grid codec");
  if (
    header.terminalHandle === 0n ||
    header.generation === 0 ||
    header.rows === 0 ||
    header.cols === 0 ||
    header.viewId === 0
  )
    throw new YasProtocolError("YASREC1 header contains a zero required field");
  if (header.ticksPerSecond !== YASREC1_TICKS_PER_SECOND)
    throw new YasProtocolError("unsupported YASREC1 timestamp timebase");

  const frames: YasTerminalRecordingFrame[] = [];
  let expectedSequence = header.firstSequence;
  let previousTimestamp: bigint | undefined;
  while (cursor.remaining !== 0) {
    if (frames.length >= maxFrames)
      throw new YasProtocolError("YASREC1 recording exceeds its frame limit");
    const timestampTicks = cursor.u64("YASREC1 frame timestamp");
    if (previousTimestamp !== undefined && timestampTicks < previousTimestamp)
      throw new YasProtocolError("YASREC1 frame timestamps went backwards");
    const frameLength = cursor.u32("YASREC1 TerminalFrame length");
    if (frameLength === 0 || frameLength > maxFrameBytes)
      throw new YasProtocolError("invalid YASREC1 TerminalFrame length");
    const frame = decodeTerminalFrame(
      cursor.take(frameLength, "YASREC1 TerminalFrame"),
    );
    if (frame.viewId !== header.viewId)
      throw new YasProtocolError(
        "YASREC1 TerminalFrame belongs to a different view",
      );
    if (frame.sequence !== expectedSequence)
      throw new YasProtocolError(
        `YASREC1 TerminalFrame sequence ${frame.sequence}, expected ${expectedSequence}`,
      );
    if (
      frames.length === 0 &&
      (frame.flags & YAS_TERMINAL_FRAME_KEYFRAME) === 0
    )
      throw new YasProtocolError(
        "YASREC1 recording must begin with a Terminal keyframe",
      );
    frames.push({ timestampTicks, frame });
    previousTimestamp = timestampTicks;
    expectedSequence = (expectedSequence + 1) >>> 0;
  }
  return Object.freeze({
    header: Object.freeze(header),
    frames: Object.freeze(frames),
  });
}

function boundedOption(
  value: number | undefined,
  fallback: number,
  name: string,
): number {
  const resolved = value ?? fallback;
  if (!Number.isSafeInteger(resolved) || resolved <= 0)
    throw new YasProtocolError(`invalid ${name}`);
  return resolved;
}

function startsWith(bytes: Uint8Array, prefix: Uint8Array): boolean {
  if (bytes.length < prefix.length) return false;
  for (let index = 0; index < prefix.length; index++)
    if (bytes[index] !== prefix[index]) return false;
  return true;
}
