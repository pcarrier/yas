/** Native YAS Terminal replay transport used by the yas.run hero. */

import {
  YAS_CLASS_EVENT,
  YAS_CLASS_REQUEST,
  YAS_CLASS_RESULT,
  YAS_CORE_HELLO,
  YAS_CORE_VERSION,
  YAS_DIRECTION_SERVER_ACCEPTS,
  YAS_DIRECTION_SERVER_SENDS,
  YAS_FAMILY_CORE,
  YAS_FAMILY_LIMIT_POLICIES,
  YAS_FAMILY_TERMINAL,
  YAS_FAMILY_TRANSFER,
  YAS_PREFACE,
  YAS_RUNTIME_AVAILABLE,
  YAS_STATE_ADD,
  YAS_STATE_SNAPSHOT_BEGIN,
  YAS_STATE_SNAPSHOT_END,
  YAS_STATE_SNAPSHOT_RECORDS,
  YAS_STATUS_NOT_FOUND,
  YAS_STATUS_OK,
  YAS_STATUS_UNSUPPORTED,
  YAS_TERMINAL_CLOSE_VIEW,
  YAS_TERMINAL_CONFIGURE_VIEW,
  YAS_TERMINAL_FRAME,
  YAS_TERMINAL_FRAME_ACK,
  YAS_TERMINAL_FRAME_EXPLICIT_BASE,
  YAS_TERMINAL_INPUT,
  YAS_TERMINAL_LIFECYCLE_RUNNING,
  YAS_TERMINAL_MOUSE,
  YAS_TERMINAL_OPEN_VIEW,
  YAS_TERMINAL_RESIZE,
  YAS_TERMINAL_SET_FOCUS,
  YAS_TERMINAL_STATE,
  YAS_TERMINAL_STATE_ACK,
  YAS_TERMINAL_STATE_RESOURCE_TAG_EXTENSION,
  YAS_TERMINAL_UNWATCH,
  YAS_TERMINAL_VERSION,
  YAS_TERMINAL_WATCH,
  YAS_TERMINAL_WHEEL,
  YAS_TERMINAL_WRITE,
  YAS_TRANSFER_VERSION,
  YAS_WATCH_MODE_SNAPSHOT,
  YasWriter,
  decodeYasFrame,
  decodeYasTerminalRecording,
  encodeExtensions,
  encodeResultPayload,
  encodeServerHello,
  encodeTypedRecord,
  encodeYasFrame,
  type ConnectionStatus,
  type YasFamilyDescriptor,
  type YasTerminalFrameEvent,
  type YasTransport,
  type YasTransportMessage,
} from "@yas-run/core";

const MAX_REPLAY_BYTES = 256 * 1024 * 1024;
const MAX_REPLAY_FRAMES = 100_000;
const REPLAY_SUBSCRIPTION_ID = 1;
const REPLAY_REVISION = 1n;
const REPLAY_VIEW_MAX_DECODED = 32 * 1024;
const REPLAY_VIEW_MAX_INFLIGHT = 255;
const encoder = new TextEncoder();

export interface ReplayFrame {
  /** Microseconds since the recording started. */
  t: number;
  frame: YasTerminalFrameEvent;
}

export interface ReplayRecording {
  terminalHandle: bigint;
  generation: number;
  rows: number;
  cols: number;
  firstSequence: number;
  frames: ReplayFrame[];
}

export interface ReplayStream extends ReplayRecording {
  tag: string;
}

/** Decode a native YASREC1 recording. */
export function parseYasrec(buf: ArrayBuffer): ReplayRecording {
  const recording = decodeYasTerminalRecording(buf, {
    maxBytes: MAX_REPLAY_BYTES,
    maxFrames: MAX_REPLAY_FRAMES,
  });
  const frames = recording.frames.map(({ timestampTicks, frame }) => {
    if (timestampTicks > BigInt(Number.MAX_SAFE_INTEGER))
      throw new Error("YASREC1 timestamp exceeds the browser clock range");
    return { t: Number(timestampTicks), frame };
  });
  if (frames.length === 0) throw new Error("YASREC1 recording is empty");
  return {
    terminalHandle: recording.header.terminalHandle,
    generation: recording.header.generation,
    rows: recording.header.rows,
    cols: recording.header.cols,
    firstSequence: recording.header.firstSequence,
    frames,
  };
}

export interface ReplayOptions {
  /** Pause this long at the end before looping (ms). */
  holdMs?: number;
  /** Render the settled end state once for reduced-motion visitors. */
  static?: boolean;
}

interface ReplayTimelineFrame {
  t: number;
  source: YasTerminalFrameEvent;
  sourceFirstSequence: number;
  sourceFrameCount: number;
  viewId: number;
}

/** A tiny native YAS server over the browser transport interface. */
export class ReplayTransport implements YasTransport {
  readonly yasFraming = "message" as const;
  authRejected = false;
  readonly maxDatagramSize = 0;
  lastError: string | null = null;

  private _status: ConnectionStatus = "connecting";
  private readonly messageListeners = new Set<
    (data: YasTransportMessage) => void
  >();
  private readonly statusListeners = new Set<
    (status: ConnectionStatus) => void
  >();
  private readonly streamsByHandle: Map<bigint, ReplayStream>;
  private timeline: ReplayTimelineFrame[] = [];
  private timer: ReturnType<typeof setTimeout> | null = null;
  private cursor = 0;
  private cycle = 0;
  private startedAt = 0;
  private playing = false;
  private holding = false;
  private nextViewId = 1;
  private activeViewId: number | null = null;

  constructor(
    private readonly streams: readonly ReplayStream[],
    private readonly opts: ReplayOptions = {},
  ) {
    this.streamsByHandle = new Map(
      streams.map((stream) => [stream.terminalHandle, stream]),
    );
    if (this.streamsByHandle.size !== streams.length)
      throw new Error("replay Terminal handles are not unique");
  }

  get status(): ConnectionStatus {
    return this._status;
  }

  connect(): void {
    if (this._status === "connected") return;
    this.setStatus("connected");
  }

  send(data: Uint8Array): void {
    if (sameBytes(data, YAS_PREFACE)) return;
    if (this._status !== "connected") return;
    let request;
    try {
      request = decodeYasFrame(data);
    } catch (error) {
      this.fail(error);
      return;
    }
    if (request.class === YAS_CLASS_EVENT) return;
    if (
      request.class !== YAS_CLASS_REQUEST ||
      request.requestId === undefined
    ) {
      this.fail(new Error("replay peer received a non-request frame"));
      return;
    }
    if (request.family === YAS_FAMILY_CORE && request.kind === YAS_CORE_HELLO) {
      this.result(request, YAS_STATUS_OK, encodeReplayHello());
      return;
    }
    if (request.family !== YAS_FAMILY_TERMINAL) {
      this.result(request, YAS_STATUS_UNSUPPORTED);
      return;
    }
    if (request.kind === YAS_TERMINAL_WATCH) {
      this.result(request, YAS_STATUS_OK, encodeWatchResult());
      queueMicrotask(() => this.emitSnapshot());
      return;
    }
    if (request.kind === YAS_TERMINAL_UNWATCH) {
      this.result(request, YAS_STATUS_OK);
      return;
    }
    if (request.kind === YAS_TERMINAL_OPEN_VIEW) {
      const stream = this.streamsByHandle.get(readU64(request.payload));
      if (!stream) {
        this.result(request, YAS_STATUS_NOT_FOUND);
        return;
      }
      const viewId = this.nextViewId++;
      this.activeViewId = viewId;
      const maximumEncoded = stream.frames.reduce(
        (maximum, value) =>
          Math.max(
            maximum,
            10 +
              (value.frame.explicitBase === undefined ? 0 : 4) +
              value.frame.gridPayload.length,
          ),
        1,
      );
      this.result(
        request,
        YAS_STATUS_OK,
        new YasWriter()
          .u32(viewId)
          .u16(1)
          .u8(REPLAY_VIEW_MAX_INFLIGHT)
          .u8(0)
          .u32(maximumEncoded)
          .u32(REPLAY_VIEW_MAX_DECODED)
          .u32(stream.firstSequence)
          .bytes(encodeExtensions([]))
          .finish(),
      );
      queueMicrotask(() => this.beginView(stream, viewId));
      return;
    }
    if (request.kind === YAS_TERMINAL_CLOSE_VIEW) {
      this.activeViewId = null;
      this.pause();
      this.timeline = [];
      this.result(request, YAS_STATUS_OK);
      return;
    }
    if (
      request.kind === YAS_TERMINAL_CONFIGURE_VIEW ||
      request.kind === YAS_TERMINAL_SET_FOCUS
    ) {
      this.result(request, YAS_STATUS_OK);
      return;
    }
    if (request.kind === YAS_TERMINAL_RESIZE) {
      this.result(
        request,
        YAS_STATUS_OK,
        new YasWriter().u64(REPLAY_REVISION).finish(),
      );
      return;
    }
    this.result(request, YAS_STATUS_UNSUPPORTED);
  }

  close(): void {
    this.pause();
    this.setStatus("closed");
  }

  addEventListener(
    type: "message" | "datagram",
    listener: (data: YasTransportMessage) => void,
  ): void;
  addEventListener(
    type: "statuschange",
    listener: (status: ConnectionStatus) => void,
  ): void;
  addEventListener(
    type: "message" | "datagram" | "statuschange",
    listener:
      | ((data: YasTransportMessage) => void)
      | ((status: ConnectionStatus) => void),
  ): void {
    if (type === "message")
      this.messageListeners.add(
        listener as (data: YasTransportMessage) => void,
      );
    else if (type === "statuschange")
      this.statusListeners.add(listener as (status: ConnectionStatus) => void);
  }

  removeEventListener(
    type: "message" | "datagram",
    listener: (data: YasTransportMessage) => void,
  ): void;
  removeEventListener(
    type: "statuschange",
    listener: (status: ConnectionStatus) => void,
  ): void;
  removeEventListener(
    type: "message" | "datagram" | "statuschange",
    listener:
      | ((data: YasTransportMessage) => void)
      | ((status: ConnectionStatus) => void),
  ): void {
    if (type === "message")
      this.messageListeners.delete(
        listener as (data: YasTransportMessage) => void,
      );
    else if (type === "statuschange")
      this.statusListeners.delete(
        listener as (status: ConnectionStatus) => void,
      );
  }

  play(): void {
    if (this.opts.static || this.playing || this.timeline.length === 0) return;
    this.playing = true;
    this.startedAt = performance.now() - (this.timeline[this.cursor]?.t ?? 0);
    this.tick();
  }

  pause(): void {
    this.playing = false;
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }

  position(): number {
    if (this.timeline.length === 0) return 0;
    const end = this.timeline[this.timeline.length - 1]!.t;
    if (this.opts.static || this.holding) return end;
    if (!this.playing)
      return this.cursor > 0
        ? Math.min(this.timeline[this.cursor - 1]!.t, end)
        : 0;
    return Math.max(0, Math.min(performance.now() - this.startedAt, end));
  }

  private beginView(stream: ReplayStream, viewId: number): void {
    if (this.activeViewId !== viewId || this._status !== "connected") return;
    this.pause();
    this.cursor = 0;
    this.cycle = 0;
    this.holding = false;
    this.timeline = stream.frames.map(({ t, frame }) => ({
      t: t / 1000,
      source: frame,
      sourceFirstSequence: stream.firstSequence,
      sourceFrameCount: stream.frames.length,
      viewId,
    }));
    if (this.opts.static) {
      for (const item of this.timeline) this.emitTimelineFrame(item);
    } else {
      this.play();
    }
  }

  private tick = (): void => {
    this.timer = null;
    if (!this.playing) return;
    const now = performance.now() - this.startedAt;
    if (now >= 0) this.holding = false;
    while (
      this.cursor < this.timeline.length &&
      this.timeline[this.cursor]!.t <= now
    ) {
      this.emitTimelineFrame(this.timeline[this.cursor]!);
      this.cursor++;
    }
    if (this.cursor >= this.timeline.length) {
      this.cursor = 0;
      this.cycle++;
      this.holding = true;
      this.startedAt = performance.now() + (this.opts.holdMs ?? 4000);
      this.timer = setTimeout(this.tick, this.opts.holdMs ?? 4000);
      return;
    }
    this.timer = setTimeout(
      this.tick,
      Math.max(0, this.timeline[this.cursor]!.t - now),
    );
  };

  private emitTimelineFrame(item: ReplayTimelineFrame): void {
    if (this.activeViewId !== item.viewId) return;
    const sourceOffset =
      (item.source.sequence - item.sourceFirstSequence) >>> 0;
    const cycleOffset = this.cycle * item.sourceFrameCount;
    const sequence =
      (item.sourceFirstSequence + cycleOffset + sourceOffset) >>> 0;
    const explicitBase =
      item.source.explicitBase === undefined
        ? undefined
        : (item.source.explicitBase + cycleOffset) >>> 0;
    const payload = new YasWriter()
      .u32(item.viewId)
      .u32(sequence)
      .u16(item.source.flags);
    if (item.source.flags & YAS_TERMINAL_FRAME_EXPLICIT_BASE)
      payload.u32(explicitBase!);
    payload.bytes(item.source.gridPayload);
    this.emit(
      encodeYasFrame({
        family: YAS_FAMILY_TERMINAL,
        kind: YAS_TERMINAL_FRAME,
        class: YAS_CLASS_EVENT,
        sensitive: true,
        payload: payload.finish(),
      }),
    );
  }

  private emitSnapshot(): void {
    if (this._status !== "connected") return;
    this.emitState(YAS_STATE_SNAPSHOT_BEGIN, 0n, REPLAY_REVISION, []);
    const records = this.streams.map((stream) => ({
      kind: YAS_STATE_ADD,
      flags: 0,
      body: new YasWriter()
        .u64(stream.terminalHandle)
        .u8(YAS_TERMINAL_LIFECYCLE_RUNNING)
        .u8(0)
        .u16(stream.rows)
        .u16(stream.cols)
        .u32(stream.generation)
        .u32(stream.rows)
        .bytes(
          encodeExtensions([
            {
              tag: YAS_TERMINAL_STATE_RESOURCE_TAG_EXTENSION,
              required: false,
              value: encoder.encode(stream.tag),
            },
          ]),
        )
        .finish(),
    }));
    this.emitState(
      YAS_STATE_SNAPSHOT_RECORDS,
      REPLAY_REVISION,
      REPLAY_REVISION,
      records,
    );
    this.emitState(
      YAS_STATE_SNAPSHOT_END,
      REPLAY_REVISION,
      REPLAY_REVISION,
      [],
    );
  }

  private emitState(
    phase: number,
    fromRevision: bigint,
    toRevision: bigint,
    records: readonly { kind: number; flags: number; body: Uint8Array }[],
  ): void {
    const payload = new YasWriter()
      .u32(REPLAY_SUBSCRIPTION_ID)
      .u8(phase)
      .u8(0)
      .u16(0)
      .u64(fromRevision)
      .u64(toRevision)
      .u16(records.length);
    for (const record of records) payload.bytes(encodeTypedRecord(record));
    this.emit(
      encodeYasFrame({
        family: YAS_FAMILY_TERMINAL,
        kind: YAS_TERMINAL_STATE,
        class: YAS_CLASS_EVENT,
        sensitive: true,
        payload: payload.finish(),
      }),
    );
  }

  private result(
    request: ReturnType<typeof decodeYasFrame>,
    status: number,
    body: Uint8Array<ArrayBufferLike> = new Uint8Array(0),
  ): void {
    this.emit(
      encodeYasFrame({
        family: request.family,
        kind: request.kind,
        class: YAS_CLASS_RESULT,
        requestId: request.requestId!,
        sensitive: request.sensitive,
        payload: encodeResultPayload(status, body),
      }),
    );
  }

  private emit(message: Uint8Array): void {
    for (const listener of this.messageListeners) listener(message);
  }

  private setStatus(status: ConnectionStatus): void {
    this._status = status;
    for (const listener of this.statusListeners) listener(status);
  }

  private fail(error: unknown): void {
    this.lastError = error instanceof Error ? error.message : String(error);
    this.setStatus("error");
    this.pause();
  }
}

function encodeReplayHello(): Uint8Array {
  return encodeServerHello({
    minor: 0,
    bootId: identity(1),
    sessionId: identity(2),
    receiveMaxFrame: 1024 * 1024,
    receiveMaxDecoded: 4 * 1024 * 1024,
    receiveMaxDatagram: 0,
    receiveMaxBuffered: 16n * 1024n * 1024n,
    serverMonotonicNs: 1n,
    catalogRevision: 1n,
    serverName: "yas.run replay",
    serverRelease: "native-demo",
    families: [
      descriptor(YAS_FAMILY_CORE, YAS_CORE_VERSION, [
        operation(
          YAS_DIRECTION_SERVER_ACCEPTS,
          YAS_CLASS_REQUEST,
          YAS_CORE_HELLO,
        ),
      ]),
      descriptor(YAS_FAMILY_TRANSFER, YAS_TRANSFER_VERSION, []),
      descriptor(YAS_FAMILY_TERMINAL, YAS_TERMINAL_VERSION, [
        operation(
          YAS_DIRECTION_SERVER_ACCEPTS,
          YAS_CLASS_REQUEST,
          YAS_TERMINAL_WATCH,
        ),
        operation(
          YAS_DIRECTION_SERVER_ACCEPTS,
          YAS_CLASS_REQUEST,
          YAS_TERMINAL_UNWATCH,
        ),
        operation(
          YAS_DIRECTION_SERVER_ACCEPTS,
          YAS_CLASS_REQUEST,
          YAS_TERMINAL_RESIZE,
        ),
        operation(
          YAS_DIRECTION_SERVER_ACCEPTS,
          YAS_CLASS_REQUEST,
          YAS_TERMINAL_SET_FOCUS,
        ),
        operation(
          YAS_DIRECTION_SERVER_ACCEPTS,
          YAS_CLASS_REQUEST,
          YAS_TERMINAL_OPEN_VIEW,
        ),
        operation(
          YAS_DIRECTION_SERVER_ACCEPTS,
          YAS_CLASS_REQUEST,
          YAS_TERMINAL_CONFIGURE_VIEW,
        ),
        operation(
          YAS_DIRECTION_SERVER_ACCEPTS,
          YAS_CLASS_REQUEST,
          YAS_TERMINAL_CLOSE_VIEW,
        ),
        operation(
          YAS_DIRECTION_SERVER_SENDS,
          YAS_CLASS_EVENT,
          YAS_TERMINAL_STATE,
        ),
        operation(
          YAS_DIRECTION_SERVER_ACCEPTS,
          YAS_CLASS_EVENT,
          YAS_TERMINAL_STATE_ACK,
        ),
        operation(
          YAS_DIRECTION_SERVER_ACCEPTS,
          YAS_CLASS_EVENT,
          YAS_TERMINAL_INPUT,
        ),
        operation(
          YAS_DIRECTION_SERVER_ACCEPTS,
          YAS_CLASS_EVENT,
          YAS_TERMINAL_MOUSE,
        ),
        operation(
          YAS_DIRECTION_SERVER_ACCEPTS,
          YAS_CLASS_EVENT,
          YAS_TERMINAL_WHEEL,
        ),
        operation(
          YAS_DIRECTION_SERVER_ACCEPTS,
          YAS_CLASS_EVENT,
          YAS_TERMINAL_WRITE,
        ),
        operation(
          YAS_DIRECTION_SERVER_SENDS,
          YAS_CLASS_EVENT,
          YAS_TERMINAL_FRAME,
        ),
        operation(
          YAS_DIRECTION_SERVER_ACCEPTS,
          YAS_CLASS_EVENT,
          YAS_TERMINAL_FRAME_ACK,
        ),
      ]),
    ],
    extensions: [],
  });
}

function descriptor(
  family: number,
  version: number,
  operations: YasFamilyDescriptor["operations"],
): YasFamilyDescriptor {
  const limits = (YAS_FAMILY_LIMIT_POLICIES[family] ?? []).map(
    ([tag, width, , , hardMaximum]) => ({
      tag,
      required: false,
      value:
        width === 4
          ? new YasWriter().u32(Number(hardMaximum)).finish()
          : new YasWriter().u64(hardMaximum).finish(),
    }),
  );
  return {
    family,
    version,
    runtimeState: YAS_RUNTIME_AVAILABLE,
    operations,
    limits,
  };
}

function operation(direction: number, frameClass: number, kind: number) {
  return { direction, class: frameClass, kind };
}

function encodeWatchResult(): Uint8Array {
  return new YasWriter()
    .u32(REPLAY_SUBSCRIPTION_ID)
    .u8(YAS_WATCH_MODE_SNAPSHOT)
    .bytes(new Uint8Array(3))
    .u64(REPLAY_REVISION)
    .bytes(encodeExtensions([]))
    .finish();
}

function identity(lastByte: number): Uint8Array {
  const value = new Uint8Array(16);
  value[15] = lastByte;
  return value;
}

function readU64(bytes: Uint8Array): bigint {
  if (bytes.length < 8) return 0n;
  return new DataView(bytes.buffer, bytes.byteOffset, 8).getBigUint64(0, true);
}

function sameBytes(left: Uint8Array, right: Uint8Array): boolean {
  return (
    left.length === right.length &&
    left.every((value, index) => value === right[index])
  );
}
