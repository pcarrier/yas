import {
  releaseRecordingAudioSession,
  retainRecordingAudioSession,
} from "./audioSession";
import { Notifier, type ReactiveStore } from "./reactive";
import type { SurfaceId } from "./types";
import { av1LevelString } from "./videoCodec";

export const RUNTIME_PIPEWIRE = 1 << 0;
export const RUNTIME_MICROPHONE = 1 << 1;
export const RUNTIME_CAMERA = 1 << 2;
export const RUNTIME_PORTAL_FRONTEND = 1 << 3;
export const RUNTIME_PORTAL_ACCESS = 1 << 4;
export const RUNTIME_PORTAL_SCREENCAST = 1 << 5;
export const RUNTIME_MPRIS = 1 << 6;

export const ACTIVE_MICROPHONE = 1 << 0;
export const ACTIVE_CAMERA = 1 << 1;
export const ACTIVE_SCREENCAST = 1 << 2;

export const AUDIO_CODEC_PCM = 1 << 0;
export const AUDIO_CODEC_OPUS = 1 << 1;
export const VIDEO_CODEC_MJPEG = 1 << 0;
export const VIDEO_CODEC_H264 = 1 << 1;
export const VIDEO_CODEC_AV1 = 1 << 2;
export const VIDEO_CODEC_H264_444 = 1 << 3;
export const VIDEO_CODEC_AV1_444 = 1 << 4;
export const VIDEO_CODECS_BASELINE = VIDEO_CODEC_MJPEG | VIDEO_CODEC_H264;
export const VIDEO_CODECS_ALL =
  VIDEO_CODECS_BASELINE |
  VIDEO_CODEC_AV1 |
  VIDEO_CODEC_H264_444 |
  VIDEO_CODEC_AV1_444;

export const MPRIS_STRING_MAX = 4 * 1024;

export const MPRIS_CAN_CONTROL = 1 << 0;
export const MPRIS_CAN_PLAY = 1 << 1;
export const MPRIS_CAN_PAUSE = 1 << 2;
export const MPRIS_CAN_GO_NEXT = 1 << 3;
export const MPRIS_CAN_GO_PREVIOUS = 1 << 4;
export const MPRIS_CAN_SEEK = 1 << 5;
export const MPRIS_CAN_RAISE = 1 << 6;
export const MPRIS_CAN_SET_VOLUME = 1 << 7;
export const MPRIS_CAN_SET_SHUFFLE = 1 << 8;
export const MPRIS_CAN_SET_LOOP_STATUS = 1 << 9;
export const MPRIS_CAN_SET_RATE = 1 << 10;

/** Opaque Media resource identity used by presentation stores. */
export type MediaId = bigint;
export type MediaRevision = bigint;

const CAMERA_FRAME_MAX = 4 * 1024 * 1024;

export interface MediaCapabilities {
  microphone: boolean;
  camera: boolean;
  portalUi: boolean;
  audioCodecs: number;
  videoCodecs: number;
  maxWidth: number;
  maxHeight: number;
  maxFps: number;
}

export interface ScreenCastState {
  sessionId: MediaId;
  appId: string;
  surfaceIds: readonly SurfaceId[];
}

/** Exact media-device owner identity: native session hex or opaque handle. */
export type MediaOwnerId = string | bigint;

export interface DesktopMediaState {
  runtimeFlags: number;
  activeFlags: number;
  microphoneOwner: MediaOwnerId;
  cameraOwner: MediaOwnerId;
  screencasts: readonly ScreenCastState[];
}

export type MediaLeaseStatus = "inactive" | "starting" | "active";

export interface MediaLeaseState {
  kind: "microphone" | "camera";
  status: MediaLeaseStatus;
  leaseId: MediaId;
  codec: number;
  width: number;
  height: number;
  fps: number;
  credit: number;
  error: string | null;
}

export interface MicrophoneOptions {
  /** Defaults to Opus when WebCodecs can encode it, otherwise PCM. */
  codec?: "pcm" | "opus";
}

export interface CameraOptions {
  /**
   * Omit to choose the best exact format supported by this browser. `h264`
   * and `av1` retain their 8-bit 4:2:0 meaning unless `chroma` is explicit.
   */
  codec?: "mjpeg" | "h264" | "av1";
  /** Exact chroma sampling for H.264/AV1. Motion JPEG does not expose this. */
  chroma?: "420" | "444";
  width?: number;
  height?: number;
  fps?: number;
  /**
   * How many bits the picture is worth. Scales the computed bitrate for the
   * compressed codecs and the JPEG quantizer for Motion JPEG — the same
   * intent expressed in whichever currency the codec takes.
   */
  quality?: CameraQuality;
}

export type CameraQuality = "low" | "balanced" | "high";

/** Bitrate multiplier and JPEG quality per quality step. */
const CAMERA_QUALITY: Record<CameraQuality, { scale: number; jpeg: number }> = {
  low: { scale: 0.5, jpeg: 0.6 },
  balanced: { scale: 1, jpeg: 0.8 },
  high: { scale: 2, jpeg: 0.92 },
};

export function cameraQuality(quality: CameraQuality | undefined) {
  return CAMERA_QUALITY[quality ?? "balanced"] ?? CAMERA_QUALITY.balanced;
}

export interface PortalChoiceValue {
  id: string;
  value: string;
}

export interface PortalChoice {
  id: string;
  label: string;
  options: readonly PortalChoiceValue[];
  initialValue: string;
}

interface PortalRequestBase {
  requestId: MediaId;
  deadlineMs: number;
  parentSurfaceId: SurfaceId | null;
  appId: string;
}

export interface PortalAccessRequest extends PortalRequestBase {
  kind: "access";
  title: string;
  subtitle: string;
  body: string;
  denyLabel: string;
  grantLabel: string;
  iconName: string;
  choices: readonly PortalChoice[];
}

export interface ScreenCastCandidate {
  surfaceId: SurfaceId;
  width: number;
  height: number;
  title: string;
  appId: string;
  thumbnailPng: Uint8Array;
}

export interface PortalScreenCastRequest extends PortalRequestBase {
  kind: "screencast";
  multiple: boolean;
  candidates: readonly ScreenCastCandidate[];
}

export type PortalRequest = PortalAccessRequest | PortalScreenCastRequest;

export interface NativeMediaController {
  startMicrophone(
    track: MediaStreamTrack,
    options: MicrophoneOptions,
  ): Promise<void>;
  startCamera(track: MediaStreamTrack, options: CameraOptions): Promise<void>;
  stop(kind: "microphone" | "camera"): void;
  reply(
    request: PortalRequest,
    decision: "deny" | "grant" | "cancelled",
    surfaceIds: readonly SurfaceId[],
    choices: readonly PortalChoiceValue[],
  ): Promise<void>;
  stopScreenCast(sessionId: bigint): Promise<void>;
}

export type PlaybackStatus = "stopped" | "paused" | "playing";
export type LoopStatus = "none" | "track" | "playlist";

/**
 * How a player's cover arrives.
 *
 * Catalogue-backed players (Spotify and friends) name their cover with an
 * `https:` URL and keep no local copy, so the server forwards that URL and the
 * browser loads and caches it: re-encoding it server-side would put ~150 KiB of
 * PNG in every upsert. Art that exists only on the server's disk cannot be
 * named to a browser, so it still arrives as bytes.
 */
export type MprisArtwork =
  | { kind: "url"; url: string }
  | { kind: "png"; png: Uint8Array };

/**
 * The only schemes this client will put in an image source. Enforced here as
 * well as on the server, because the value reaches the DOM.
 */
export function artworkUrlAllowed(url: string): boolean {
  if (url.length === 0 || url.length > MPRIS_STRING_MAX) return false;
  const separator = url.indexOf("://");
  if (separator <= 0 || separator + 3 >= url.length) return false;
  const scheme = url.slice(0, separator).toLowerCase();
  return scheme === "https" || scheme === "http";
}

export interface MprisPlayer {
  playerId: MediaId;
  revision: MediaRevision;
  trackRevision: MediaRevision;
  active: boolean;
  playbackStatus: PlaybackStatus;
  loopStatus: LoopStatus;
  shuffle: boolean;
  capabilityFlags: number;
  rate: number;
  minimumRate: number;
  maximumRate: number;
  volume: number;
  positionUs: number;
  lengthUs: number;
  identity: string;
  desktopEntry: string;
  title: string;
  album: string;
  artists: readonly string[];
  artwork: MprisArtwork | null;
  /** Local monotonic receipt anchor; never compared with a server clock. */
  receivedAtMs: number;
}

export type MprisAction =
  | { kind: "select" }
  | { kind: "play" }
  | { kind: "pause" }
  | { kind: "playPause" }
  | { kind: "stop" }
  | { kind: "next" }
  | { kind: "previous" }
  | { kind: "seek"; offsetUs: number }
  | {
      kind: "setPosition";
      positionUs: number;
      trackRevision: MediaRevision;
    }
  | { kind: "volume"; volume: number }
  | { kind: "shuffle"; shuffle: boolean }
  | { kind: "loopStatus"; loopStatus: LoopStatus }
  | { kind: "rate"; rate: number }
  | { kind: "raise" };

export interface NativeMprisController {
  subscribe(enabled: boolean): void;
  act(playerId: bigint, action: MprisAction): Promise<void>;
}

/** Browser presentation state for the native YAS MPRIS catalogue. */
export class MprisStore implements ReactiveStore {
  readonly #notifier = new Notifier();
  readonly #players = new Map<MediaId, MprisPlayer>();
  #native: NativeMprisController | null = null;
  #subscribed = false;

  get revision(): number {
    return this.#notifier.revision;
  }

  get players(): ReadonlyMap<MediaId, MprisPlayer> {
    return this.#players;
  }

  get activePlayerId(): MediaId | null {
    for (const player of this.#players.values()) {
      if (player.active) return player.playerId;
    }
    return null;
  }

  get activePlayer(): MprisPlayer | null {
    const id = this.activePlayerId;
    return id === null ? null : (this.#players.get(id) ?? null);
  }

  subscribe(listener: () => void): () => void;
  subscribe(enabled: boolean): void;
  subscribe(value: boolean | (() => void)): void | (() => void) {
    if (typeof value === "function") return this.#notifier.subscribe(value);
    this.#subscribed = value;
    this.#native?.subscribe(value);
  }

  setNativeController(controller: NativeMprisController | null): void {
    this.#native = controller;
    if (controller && this.#subscribed) controller.subscribe(true);
  }

  replaceNative(players: readonly MprisPlayer[]): void {
    this.#players.clear();
    for (const player of players) this.#players.set(player.playerId, player);
    this.#notifier.emit();
  }

  select(playerId: MediaId): Promise<void> {
    return this.act(playerId, { kind: "select" });
  }

  act(playerId: MediaId, action: MprisAction): Promise<void> {
    if (!this.#native)
      return Promise.reject(new Error("MPRIS family unavailable"));
    if (typeof playerId !== "bigint" || playerId === 0n)
      return Promise.reject(new Error("MPRIS player is not a native handle"));
    const player = this.#players.get(playerId);
    if (!player || typeof player.revision !== "bigint")
      return Promise.reject(new Error("MPRIS player no longer exists"));
    return this.#native.act(playerId, action);
  }

  positionUs(playerId: MediaId, nowMs = monotonicNow()): number {
    const player = this.#players.get(playerId);
    if (!player) return 0;
    let position = player.positionUs;
    if (player.playbackStatus === "playing")
      position +=
        Math.max(0, nowMs - player.receivedAtMs) * 1_000 * player.rate;
    if (player.lengthUs >= 0) position = Math.min(position, player.lengthUs);
    return Math.max(0, Math.round(position));
  }

  reconnect(): void {
    this.reset();
    if (this.#subscribed) this.#native?.subscribe(true);
  }

  reset(): void {
    const changed = this.#players.size > 0;
    this.#players.clear();
    if (changed) this.#notifier.emit();
  }
}

const microphoneWorklet = `
class YasMicrophoneProcessor extends AudioWorkletProcessor {
  process(inputs) {
    const channel = inputs[0]?.[0];
    if (channel?.length) {
      const copy = channel.slice();
      this.port.postMessage(copy.buffer, [copy.buffer]);
    }
    return true;
  }
}
registerProcessor("yas-microphone", YasMicrophoneProcessor);
`;

export class PcmMicrophoneCapture {
  readonly track: MediaStreamTrack;
  readonly #frame: (pcm: Uint8Array, captureUs: number) => void;
  readonly #ended: () => void;
  #context: AudioContext | null = null;
  #source: MediaStreamAudioSourceNode | null = null;
  #node: AudioWorkletNode | null = null;
  #sink: GainNode | null = null;
  #pending = new Float32Array(0);
  #cursor = 0;
  #samples: number[] = [];
  #emittedSamples = 0;
  /** Whether this capture still owns a recording claim. `stop()` runs on
   *  several paths — server revoke, device ended, teardown — and a claim
   *  released twice would strand a second capture's session on playback. */
  #recordingClaim = false;

  constructor(
    track: MediaStreamTrack,
    frame: (pcm: Uint8Array, captureUs: number) => void,
    ended: () => void,
  ) {
    this.track = track;
    this.#frame = frame;
    this.#ended = ended;
  }

  async start(): Promise<void> {
    if (this.track.kind !== "audio" || this.track.readyState !== "live") {
      throw new Error("microphone track is not live");
    }
    // Before the context exists: iOS routes Bluetooth when a capture-carrying
    // context is created, so the category has to be recording-capable by then
    // rather than once samples start flowing.
    this.#recordingClaim = true;
    retainRecordingAudioSession();
    const context = new AudioContext({ latencyHint: "interactive" });
    this.#context = context;
    const url = URL.createObjectURL(
      new Blob([microphoneWorklet], { type: "text/javascript" }),
    );
    try {
      await context.audioWorklet.addModule(url);
    } finally {
      URL.revokeObjectURL(url);
    }
    if (this.track.readyState !== "live") {
      await context.close();
      throw new Error("microphone track ended during initialization");
    }
    this.#source = context.createMediaStreamSource(
      new MediaStream([this.track]),
    );
    this.#node = new AudioWorkletNode(context, "yas-microphone", {
      numberOfInputs: 1,
      numberOfOutputs: 1,
      outputChannelCount: [1],
    });
    this.#sink = context.createGain();
    this.#sink.gain.value = 0;
    this.#node.port.onmessage = (event: MessageEvent<ArrayBuffer>) => {
      this.#push(new Float32Array(event.data), context.sampleRate);
    };
    this.#source
      .connect(this.#node)
      .connect(this.#sink)
      .connect(context.destination);
    this.track.addEventListener("ended", this.#ended, { once: true });
    await context.resume();
  }

  stop(stopTrack = true): void {
    if (this.#recordingClaim) {
      this.#recordingClaim = false;
      releaseRecordingAudioSession();
    }
    this.track.removeEventListener("ended", this.#ended);
    this.#node?.disconnect();
    this.#source?.disconnect();
    this.#sink?.disconnect();
    this.#node = null;
    this.#source = null;
    this.#sink = null;
    if (this.#context) void this.#context.close();
    this.#context = null;
    if (stopTrack) this.track.stop();
  }

  #push(input: Float32Array, sampleRate: number): void {
    const joined = new Float32Array(this.#pending.length + input.length);
    joined.set(this.#pending);
    joined.set(input, this.#pending.length);
    const step = sampleRate / 48_000;
    while (this.#cursor + 1 < joined.length) {
      const index = Math.floor(this.#cursor);
      const fraction = this.#cursor - index;
      this.#samples.push(
        joined[index]! + (joined[index + 1]! - joined[index]!) * fraction,
      );
      this.#cursor += step;
      if (this.#samples.length === 960) {
        const pcm = new Uint8Array(960 * 2);
        const view = new DataView(pcm.buffer);
        for (let i = 0; i < 960; i++) {
          const sample = Math.max(-1, Math.min(1, this.#samples[i]!));
          view.setInt16(
            i * 2,
            sample < 0
              ? Math.round(sample * 32768)
              : Math.round(sample * 32767),
            true,
          );
        }
        this.#samples.length = 0;
        const captureUs = Math.round(
          (this.#emittedSamples * 1_000_000) / 48_000,
        );
        this.#emittedSamples += 960;
        this.#frame(pcm, captureUs);
      }
    }
    const consumed = Math.floor(this.#cursor);
    this.#pending = joined.slice(consumed);
    this.#cursor -= consumed;
  }
}

type EncodedAudioChunkLike = {
  readonly byteLength: number;
  readonly timestamp: number;
  copyTo(destination: Uint8Array): void;
};
type AudioEncoderLike = {
  readonly encodeQueueSize: number;
  configure(config: object): void;
  encode(data: AudioDataLike): void;
  close(): void;
};
type AudioDataLike = { close(): void };
type AudioEncoderConstructor = {
  new (init: {
    output: (chunk: EncodedAudioChunkLike) => void;
    error: (error: DOMException) => void;
  }): AudioEncoderLike;
  isConfigSupported(config: object): Promise<{ supported?: boolean }>;
};
type AudioDataConstructor = new (init: object) => AudioDataLike;

function webCodecsAudio(): {
  Encoder: AudioEncoderConstructor;
  Data: AudioDataConstructor;
} | null {
  const globals = globalThis as typeof globalThis & {
    AudioEncoder?: AudioEncoderConstructor;
    AudioData?: AudioDataConstructor;
  };
  return globals.AudioEncoder && globals.AudioData
    ? { Encoder: globals.AudioEncoder, Data: globals.AudioData }
    : null;
}

export function supportsOpusMicrophone(): boolean {
  return webCodecsAudio() !== null;
}

let opusSupportProbe: Promise<boolean> | null = null;

function opusEncoderConfig(): object {
  return {
    codec: "opus",
    sampleRate: 48_000,
    numberOfChannels: 1,
    bitrate: 32_000,
    opus: { frameDuration: 20_000 },
  };
}

/** Performs the asynchronous WebCodecs codec check without opening a device. */
export function probeOpusMicrophone(): Promise<boolean> {
  if (opusSupportProbe) return opusSupportProbe;
  const codecs = webCodecsAudio();
  opusSupportProbe = codecs
    ? codecs.Encoder.isConfigSupported(opusEncoderConfig()).then(
        (support) => Boolean(support.supported),
        () => false,
      )
    : Promise.resolve(false);
  return opusSupportProbe;
}

export class OpusMicrophoneEncoder {
  readonly #encoder: AudioEncoderLike;
  readonly #Data: AudioDataConstructor;
  readonly #output: (packet: Uint8Array, captureUs: number) => void;

  private constructor(
    encoder: AudioEncoderLike,
    Data: AudioDataConstructor,
    output: (packet: Uint8Array, captureUs: number) => void,
  ) {
    this.#encoder = encoder;
    this.#Data = Data;
    this.#output = output;
  }

  static async create(
    output: (packet: Uint8Array, captureUs: number) => void,
    failed: (error: Error) => void,
  ): Promise<OpusMicrophoneEncoder> {
    const codecs = webCodecsAudio();
    if (!codecs) throw new Error("WebCodecs audio encoding is unavailable");
    const config = opusEncoderConfig();
    const support = await codecs.Encoder.isConfigSupported(config);
    if (!support.supported) throw new Error("This browser cannot encode Opus");
    let instance: OpusMicrophoneEncoder | null = null;
    const encoder = new codecs.Encoder({
      output: (chunk) => {
        const packet = new Uint8Array(chunk.byteLength);
        chunk.copyTo(packet);
        if (instance) instance.#output(packet, chunk.timestamp);
      },
      error: (error) => failed(error),
    });
    encoder.configure(config);
    instance = new OpusMicrophoneEncoder(encoder, codecs.Data, output);
    return instance;
  }

  encode(pcm: Uint8Array, captureUs: number): void {
    if (this.#encoder.encodeQueueSize >= 3) return;
    const data = new this.#Data({
      format: "s16",
      sampleRate: 48_000,
      numberOfFrames: 960,
      numberOfChannels: 1,
      timestamp: captureUs,
      data: pcm,
    });
    try {
      this.#encoder.encode(data);
    } finally {
      data.close();
    }
  }

  stop(): void {
    try {
      this.#encoder.close();
    } catch {
      // WebCodecs may already have closed the encoder after its error callback.
    }
  }
}

export type CameraWireCodec = 0 | 1 | 2 | 3 | 4;

const CAMERA_CODEC_AUTO_ORDER: readonly CameraWireCodec[] = [4, 2, 3, 1, 0];
const CAMERA_KEYFRAME_INTERVAL_US = 2_000_000;

export function cameraCodecBit(codec: CameraWireCodec): number {
  return 1 << codec;
}

/** Human name for a wire codec — the negotiated answer is worth showing, not
 *  just logging: "the camera is stuck on Motion JPEG" is unanswerable from a
 *  panel that only reports the size and cadence it settled on. */
export function cameraCodecLabel(codec: number): string {
  switch (codec) {
    case 0:
      return "Motion JPEG";
    case 1:
      return "H.264 4:2:0";
    case 2:
      return "AV1 4:2:0";
    case 3:
      return "H.264 4:4:4";
    case 4:
      return "AV1 4:4:4";
    // A lease's codec arrives off the wire, so a caller can hold a byte this
    // build has no name for. Naming it anyway beats an empty label.
    default:
      return `codec ${codec}`;
  }
}

export function cameraCodecCandidates(
  options: CameraOptions,
): readonly CameraWireCodec[] {
  if (
    options.codec !== undefined &&
    options.codec !== "mjpeg" &&
    options.codec !== "h264" &&
    options.codec !== "av1"
  ) {
    throw new Error("unknown camera codec");
  }
  if (
    options.chroma !== undefined &&
    options.chroma !== "420" &&
    options.chroma !== "444"
  ) {
    throw new Error("unknown camera chroma format");
  }
  if (options.codec === "mjpeg") {
    if (options.chroma !== undefined) {
      throw new Error("Motion JPEG does not expose an exact chroma selection");
    }
    return [0];
  }
  if (options.codec === "h264") return [options.chroma === "444" ? 3 : 1];
  if (options.codec === "av1") return [options.chroma === "444" ? 4 : 2];
  if (options.chroma === "444") return [4, 3];
  if (options.chroma === "420") return [2, 1];
  return CAMERA_CODEC_AUTO_ORDER;
}

function h264CameraLevel(width: number, height: number): string {
  return width <= 1280 && height <= 720 ? "1f" : "28";
}

/** Bits each codec spends per pixel per frame, before any quality scale. */
function cameraBitsPerPixel(codec: CameraWireCodec): number {
  switch (codec) {
    // Motion JPEG configures no bitrate: every picture is a whole intra
    // frame, and this is what one costs.
    case 0:
      return 1.2;
    case 1:
      return 0.11;
    case 2:
      return 0.075;
    case 3:
      return 0.16;
    case 4:
      return 0.11;
  }
}

/**
 * Bytes per second this camera configuration is expected to produce.
 *
 * The server sizes the lease window from the same arithmetic, so the two
 * agree on what a second of video costs — keep them in step.
 */
export function cameraBytesPerSecond(
  codec: CameraWireCodec,
  width: number,
  height: number,
  fps: number,
  scale = 1,
): number {
  const bits = width * height * fps * cameraBitsPerPixel(codec) * scale;
  return Math.max(0, bits / 8);
}

/**
 * Chooses how hard the camera encoder should push, from whether the link is
 * keeping up.
 *
 * Dropping frames keeps the picture current but spends the whole shortfall
 * on stutter; encoding smaller frames instead spends it on detail, which is
 * the better trade for a webcam. So congestion should lower the bitrate, not
 * just thin the stream.
 *
 * The two arms are deliberately asymmetric in speed but both present: back
 * off quickly, because the delay is already being felt, and recover slowly,
 * because probing upward costs another round of congestion when it is wrong.
 * An arm that can only ever degrade is the failure this is written against —
 * a link that recovers has to be able to earn its quality back, or one bad
 * minute quietly sets the quality for the rest of the session.
 */
export class CameraRateGovernor {
  static readonly MIN_SCALE = 0.25;
  static readonly MAX_SCALE = 1;
  static readonly BACKOFF = 0.75;
  static readonly RECOVER = 1.15;
  /** Consecutive clear intervals required before probing upward again. */
  static readonly RECOVER_AFTER = 5;

  #scale = 1;
  #clear = 0;

  get scale(): number {
    return this.#scale;
  }

  /** Fold in one observation interval; returns the scale to encode at. */
  observe(congested: boolean): number {
    if (congested) {
      this.#clear = 0;
      this.#scale = Math.max(
        CameraRateGovernor.MIN_SCALE,
        this.#scale * CameraRateGovernor.BACKOFF,
      );
      return this.#scale;
    }
    this.#clear += 1;
    if (this.#clear >= CameraRateGovernor.RECOVER_AFTER) {
      this.#clear = 0;
      this.#scale = Math.min(
        CameraRateGovernor.MAX_SCALE,
        this.#scale * CameraRateGovernor.RECOVER,
      );
    }
    return this.#scale;
  }

  reset(): void {
    this.#scale = 1;
    this.#clear = 0;
  }
}

function cameraEncoderConfig(
  codec: Exclude<CameraWireCodec, 0>,
  width: number,
  height: number,
  fps: number,
  /** Quality multiplier on the computed bitrate; 1 is the balanced default.
   *  The support probe leaves it at 1 — a codec is not supported or not
   *  supported at a different bitrate. */
  scale = 1,
): VideoEncoderConfig {
  const av1 = codec === 2 || codec === 4;
  const chroma444 = codec === 3 || codec === 4;
  const bitsPerPixel = av1
    ? chroma444
      ? 0.11
      : 0.075
    : chroma444
      ? 0.16
      : 0.11;
  const bitrate = Math.max(
    150_000,
    Math.min(
      8_000_000,
      Math.round(width * height * fps * bitsPerPixel * scale),
    ),
  );
  return {
    codec: av1
      ? `av01.${chroma444 ? 1 : 0}.${av1LevelString(width, height)}M.08`
      : `avc1.${chroma444 ? "F400" : "4200"}${h264CameraLevel(width, height)}`,
    width,
    height,
    displayWidth: width,
    displayHeight: height,
    framerate: fps,
    bitrate,
    bitrateMode: "variable",
    latencyMode: "realtime",
    hardwareAcceleration: "no-preference",
    ...(av1 ? {} : { avc: { format: "annexb" as const } }),
  };
}

type H264NalRange = {
  start: number;
  end: number;
  nal: number;
  kind: number;
};

function h264NalRanges(data: Uint8Array): H264NalRange[] {
  const ranges: H264NalRange[] = [];
  let offset = 0;
  while (offset + 3 < data.length) {
    let start = -1;
    let prefix = 0;
    for (let i = offset; i + 3 < data.length; i++) {
      if (data[i] !== 0 || data[i + 1] !== 0) continue;
      if (data[i + 2] === 1) {
        start = i;
        prefix = 3;
        break;
      }
      if (i + 3 < data.length && data[i + 2] === 0 && data[i + 3] === 1) {
        start = i;
        prefix = 4;
        break;
      }
    }
    if (start < 0) break;
    if (ranges.length) ranges[ranges.length - 1]!.end = start;
    const nal = start + prefix;
    if (nal >= data.length) break;
    ranges.push({ start, end: data.length, nal, kind: data[nal]! & 0x1f });
    offset = nal + 1;
  }
  return ranges;
}

function h264SpsFormat(
  data: Uint8Array,
): { profile: number; chromaFormat: number } | null {
  const ranges = h264NalRanges(data);
  if (
    !ranges.some((range) => range.kind === 8) ||
    !ranges.some((range) => range.kind === 5)
  ) {
    return null;
  }
  const sps = ranges.find((range) => range.kind === 7);
  if (!sps) return null;
  const escaped = data.subarray(sps.nal + 1, sps.end);
  const rbsp: number[] = [];
  let zeros = 0;
  for (const byte of escaped) {
    if (zeros >= 2 && byte === 3) {
      continue;
    }
    rbsp.push(byte);
    zeros = byte === 0 ? zeros + 1 : 0;
  }
  if (rbsp.length < 3) return null;
  const profile = rbsp[0]!;
  const highProfiles = new Set([
    100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134, 135,
  ]);
  if (!highProfiles.has(profile)) return { profile, chromaFormat: 1 };
  let bit = 24;
  const readBit = (): number | null => {
    if (bit >= rbsp.length * 8) return null;
    const value = (rbsp[bit >>> 3]! >>> (7 - (bit & 7))) & 1;
    bit++;
    return value;
  };
  const readUe = (): number | null => {
    let zeros = 0;
    for (;;) {
      const value = readBit();
      if (value === null || zeros > 30) return null;
      if (value === 1) break;
      zeros++;
    }
    let suffix = 0;
    for (let index = 0; index < zeros; index++) {
      const value = readBit();
      if (value === null) return null;
      suffix = (suffix << 1) | value;
    }
    return 2 ** zeros - 1 + suffix;
  };
  if (readUe() === null) return null; // seq_parameter_set_id
  const chromaFormat = readUe();
  return chromaFormat !== null && chromaFormat <= 3
    ? { profile, chromaFormat }
    : null;
}

function h264ParameterSets(data: Uint8Array): Uint8Array | null {
  const ranges = h264NalRanges(data);
  const selected = ranges.filter(
    (range) => range.kind === 7 || range.kind === 8,
  );
  if (
    !selected.some((range) => range.kind === 7) ||
    !selected.some((range) => range.kind === 8)
  ) {
    return null;
  }
  const length = selected.reduce(
    (sum, range) => sum + range.end - range.start,
    0,
  );
  const out = new Uint8Array(length);
  let cursor = 0;
  for (const range of selected) {
    const nal = data.subarray(range.start, range.end);
    out.set(nal, cursor);
    cursor += nal.length;
  }
  return out;
}

type Av1SequenceHeader = {
  profile: number;
  obu: Uint8Array;
};

function av1SequenceHeader(data: Uint8Array): Av1SequenceHeader | null {
  let offset = 0;
  while (offset < data.length) {
    const start = offset;
    const header = data[offset++]!;
    if (header & 0x81) return null;
    const type = (header >>> 3) & 0x0f;
    if (header & 0x04) {
      if (offset >= data.length) return null;
      offset++;
    }
    let size = data.length - offset;
    if (header & 0x02) {
      size = 0;
      let shift = 0;
      for (;;) {
        if (offset >= data.length || shift > 28) return null;
        const byte = data[offset++]!;
        size |= (byte & 0x7f) << shift;
        if (!(byte & 0x80)) break;
        shift += 7;
      }
    }
    if (offset + size > data.length) return null;
    const end = offset + size;
    if (type === 1) {
      // A sequence header must at least carry `seq_profile`.
      if (size < 1) return null;
      return {
        profile: data[offset]! >>> 5,
        obu: data.slice(start, end),
      };
    }
    // Zero-length OBUs are legal and routine — every AV1 encoder opens a
    // temporal unit with a payload-less temporal delimiter. Rejecting them
    // (this used to be `size <= 0`) meant the walk gave up on the very first
    // OBU of every real keyframe and never saw the sequence header behind it,
    // so yas read its own AV1 camera streams as the wrong profile and refused
    // them, in the probe and again in the capture.
    offset = end;
  }
  return null;
}

function av1SequenceProfile(data: Uint8Array): number | null {
  return av1SequenceHeader(data)?.profile ?? null;
}

function encodedCameraProfileMatches(
  codec: Exclude<CameraWireCodec, 0>,
  chunk: EncodedVideoChunk,
): boolean {
  if (chunk.type !== "key" || chunk.byteLength === 0) return false;
  const data = new Uint8Array(chunk.byteLength);
  chunk.copyTo(data);
  return cameraBitstreamMatchesCodec(codec, data);
}

/**
 * Whether a keyframe's bitstream carries what the wire codec promises.
 *
 * Split out from the probe so it can be tested without a `VideoEncoder`: the
 * rule it encodes is the whole reason a browser keeps or loses a codec.
 */
export function cameraBitstreamMatchesCodec(
  codec: Exclude<CameraWireCodec, 0>,
  data: Uint8Array,
): boolean {
  if (codec === 1 || codec === 3) {
    // Chroma, not profile. The wire codec distinguishes 4:2:0 from 4:4:4 and
    // nothing else — the server maps it to `(H264, Cs420)` and hands the
    // bitstream to a decoder that reads the profile out of the SPS like any
    // other. Requiring the exact profile we *asked* for rejects encoders that
    // honour the request with a superset: VideoToolbox answers a Baseline
    // request with Main or High, so Safari on macOS failed this probe, lost
    // H.264 and AV1, and fell back to Motion JPEG — a whole intra frame per
    // picture — for a stream it could have encoded properly all along.
    const format = h264SpsFormat(data);
    return format?.chromaFormat === (codec === 3 ? 3 : 1);
  }
  return av1SequenceProfile(data) === (codec === 4 ? 1 : 0);
}

/** What a keyframe leaving the encoder is worth on the wire. `header`, when
 *  present, is the parameter-set prefix to remember for the keyframes that
 *  arrive without one. */
export type CameraKeyframeDecision =
  | { action: "send"; data: Uint8Array; header: Uint8Array | null }
  | { action: "reject" }
  | { action: "drop" };

/**
 * Prepare an encoded keyframe for the wire, or refuse it.
 *
 * The server decodes from a keyframe alone, so one must arrive self-contained:
 * an encoder that emits its parameter sets once has them prepended from
 * `cachedHeader`, and a keyframe with neither is dropped rather than sent as a
 * picture nothing can start from.
 *
 * The format check is deliberately the *same* rule the support probe uses —
 * [`cameraBitstreamMatchesCodec`], chroma rather than the exact profile.
 * Holding the live stream to a stricter rule than the probe is what kept macOS
 * on Motion JPEG after the probe was relaxed: VideoToolbox answers yas's
 * Baseline request with Main or High, the panel offered H.264 because the probe
 * now accepts that, and then the first keyframe of every session was rejected
 * here and took the lease down with it.
 *
 * Split out of the capture class so that rule can be tested without a
 * `VideoEncoder`, exactly as the probe's is.
 */
export function cameraKeyframeForWire(
  codec: Exclude<CameraWireCodec, 0>,
  data: Uint8Array,
  cachedHeader: Uint8Array | null,
): CameraKeyframeDecision {
  const header =
    codec === 1 || codec === 3
      ? h264ParameterSets(data)
      : (av1SequenceHeader(data)?.obu ?? null);
  if (header) {
    return cameraBitstreamMatchesCodec(codec, data)
      ? { action: "send", data, header }
      : { action: "reject" };
  }
  if (!cachedHeader) return { action: "drop" };
  const selfContained = new Uint8Array(cachedHeader.length + data.length);
  selfContained.set(cachedHeader);
  selfContained.set(data, cachedHeader.length);
  return { action: "send", data: selfContained, header: null };
}

/**
 * Sources to try for the probe's one frame, best first.
 *
 * A canvas is only a usable `VideoFrame` source once it *has* a bitmap, and an
 * `OffscreenCanvas` gets one from its first rendering context — so
 * `new VideoFrame(new OffscreenCanvas(w, h))` throws
 * `InvalidStateError: Invalid source state` (measured in Chromium 151). This
 * used to hand the probe exactly that, so the frame constructor threw for
 * *every* codec, every probe read as "this browser cannot encode it", and the
 * camera fell back to Motion JPEG in every browser that has OffscreenCanvas —
 * which is all of them. Nothing pointed at the canvas: the only symptom was a
 * panel offering one format.
 *
 * So take the context, and keep an `HTMLCanvasElement` behind it: that is what
 * the capture path itself encodes from, and it is a valid source with or
 * without a context. Returning candidates rather than one source means a host
 * that denies one kind of canvas still gets a probe.
 */
function cameraProbeSources(): (() => CanvasImageSource)[] {
  const candidates: (() => CanvasImageSource)[] = [];
  if (typeof OffscreenCanvas !== "undefined") {
    candidates.push(() => {
      const canvas = new OffscreenCanvas(320, 240);
      // Taking the context is what makes the bitmap exist, and is the whole
      // point here. The fill merely makes the probe frame deterministic
      // instead of implementation-defined, so it must not be able to fail the
      // probe: a host that hands back a restricted context still has a bitmap.
      const context = canvas.getContext("2d");
      if (!context) throw new Error("offscreen 2D context unavailable");
      context.fillRect?.(0, 0, 320, 240);
      return canvas as unknown as CanvasImageSource;
    });
  }
  if (typeof document !== "undefined") {
    candidates.push(() => {
      const canvas = document.createElement("canvas");
      canvas.width = 320;
      canvas.height = 240;
      canvas.getContext("2d")?.fillRect?.(0, 0, 320, 240);
      return canvas;
    });
  }
  return candidates;
}

/** A frame to encode, from the first source that yields one. */
function makeCameraProbeFrame(): VideoFrame | null {
  for (const source of cameraProbeSources()) {
    try {
      return new VideoFrame(source(), { timestamp: 0 });
    } catch {
      // Try the next kind of canvas: a source yas cannot build says nothing
      // about the codec, and must never be reported as if it did.
    }
  }
  return null;
}

export function supportsMjpegCamera(): boolean {
  return (
    typeof document !== "undefined" &&
    typeof document.createElement === "function" &&
    typeof HTMLCanvasElement !== "undefined" &&
    typeof HTMLCanvasElement.prototype.toBlob === "function"
  );
}

let cameraProbeEncoder: typeof VideoEncoder | undefined;
let cameraProbeFrame: typeof VideoFrame | undefined;
const cameraCodecProbes = new Map<string, Promise<CameraCodecProbeOutcome>>();

/**
 * What a camera codec probe found, kept so a UI can say which side refused.
 *
 * "This browser cannot encode it or no desktop accepts it" is not a diagnosis,
 * and a camera silently pinned to Motion JPEG is exactly the case where the
 * difference matters: `config-unsupported` is a browser with no such encoder,
 * `no-keyframe` is one that accepted the config and produced nothing (a slow
 * or wedged hardware session), and `wrong-format` is one whose bitstream does
 * not carry the chroma the wire codec promises the server.
 */
export type CameraCodecProbeOutcome =
  | "supported"
  | "no-webcodecs"
  | "no-test-frame"
  | "config-unsupported"
  | "encoder-error"
  | "no-keyframe"
  | "wrong-format";

const cameraCodecOutcomes = new Map<CameraWireCodec, CameraCodecProbeOutcome>();

/** The last probe result per wire codec. Motion JPEG never appears: it needs no
 *  encoder, and [`supportsMjpegCamera`] is the whole of its support test. */
export function cameraCodecProbeOutcomes(): ReadonlyMap<
  CameraWireCodec,
  CameraCodecProbeOutcome
> {
  return cameraCodecOutcomes;
}

/**
 * How long a probe waits for its keyframe.
 *
 * This is a *cold* encoder session on a page that is still loading, and the
 * four formats are probed one after another, so the first one pays for
 * spinning up the platform's video hardware. 1.5s was too tight for that:
 * a probe that times out is cached as an unsupported codec, and losing H.264
 * that way drops the whole camera to Motion JPEG with nothing logged. Nothing
 * waits on this — the mask is needed when the panel opens or a lease starts —
 * so the budget can afford to be generous.
 */
const CAMERA_PROBE_DEADLINE_MS = 5_000;

async function emitCameraProbeFrame(
  codec: Exclude<CameraWireCodec, 0>,
): Promise<CameraCodecProbeOutcome> {
  if (
    typeof VideoEncoder === "undefined" ||
    typeof VideoFrame === "undefined"
  ) {
    return "no-webcodecs";
  }
  // Built before the encoder so a source yas cannot construct is never
  // mistaken for a codec the browser cannot encode.
  const probeFrame = makeCameraProbeFrame();
  if (!probeFrame) return "no-test-frame";
  const config = cameraEncoderConfig(codec, 320, 240, 15);
  return new Promise<CameraCodecProbeOutcome>((resolve) => {
    let encoder: VideoEncoder | null = null;
    let settled = false;
    let valid = false;
    let sawChunk = false;
    const finish = (result: CameraCodecProbeOutcome) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      try {
        encoder?.close();
      } catch {
        // An encoder error may have closed it already.
      }
      resolve(result);
    };
    const timer = setTimeout(
      () => finish("no-keyframe"),
      CAMERA_PROBE_DEADLINE_MS,
    );
    try {
      encoder = new VideoEncoder({
        output: (chunk) => {
          sawChunk = true;
          valid ||= encodedCameraProfileMatches(codec, chunk);
        },
        error: () => finish("encoder-error"),
      });
      encoder.configure(config);
      try {
        encoder.encode(probeFrame, { keyFrame: true });
      } finally {
        probeFrame.close();
      }
      void encoder.flush().then(
        () =>
          finish(
            valid ? "supported" : sawChunk ? "wrong-format" : "no-keyframe",
          ),
        () => finish("encoder-error"),
      );
    } catch {
      finish("encoder-error");
    }
  });
}

function probeCameraCodecOutcome(
  codec: Exclude<CameraWireCodec, 0>,
  width: number,
  height: number,
  fps: number,
): Promise<CameraCodecProbeOutcome> {
  if (
    typeof VideoEncoder === "undefined" ||
    typeof VideoFrame === "undefined"
  ) {
    cameraCodecOutcomes.set(codec, "no-webcodecs");
    return Promise.resolve("no-webcodecs");
  }
  if (cameraProbeEncoder !== VideoEncoder || cameraProbeFrame !== VideoFrame) {
    cameraProbeEncoder = VideoEncoder;
    cameraProbeFrame = VideoFrame;
    cameraCodecProbes.clear();
  }
  const key = `${codec}:${width}x${height}@${fps}`;
  const cached = cameraCodecProbes.get(key);
  if (cached) return cached;
  const probe = VideoEncoder.isConfigSupported(
    cameraEncoderConfig(codec, width, height, fps),
  )
    .then((support) =>
      support.supported
        ? emitCameraProbeFrame(codec)
        : ("config-unsupported" as CameraCodecProbeOutcome),
    )
    .catch(() => "encoder-error" as CameraCodecProbeOutcome)
    .then((outcome) => {
      cameraCodecOutcomes.set(codec, outcome);
      // A codec that produced nothing in time has not answered the question,
      // so it must not be remembered as an answer: the capture path probes at
      // its own frame size and deserves a fresh attempt at a warm encoder.
      if (outcome === "no-keyframe") cameraCodecProbes.delete(key);
      return outcome;
    });
  cameraCodecProbes.set(key, probe);
  return probe;
}

export function probeCameraCodec(
  codec: Exclude<CameraWireCodec, 0>,
  width: number,
  height: number,
  fps: number,
): Promise<boolean> {
  return probeCameraCodecOutcome(codec, width, height, fps).then(
    (outcome) => outcome === "supported",
  );
}

/**
 * Probe exact camera encoder profiles. A bit is returned only after the
 * browser both accepts the requested config and emits the matching profile.
 *
 * Every codec's verdict is also recorded in [`cameraCodecProbeOutcomes`], so a
 * missing bit can be explained rather than merely reported.
 */
export async function probeCameraCodecs(
  maxWidth = 1920,
  maxHeight = 1080,
  maxFps = 30,
): Promise<number> {
  const width = Math.max(1, Math.min(1920, Math.trunc(maxWidth)));
  const height = Math.max(1, Math.min(1080, Math.trunc(maxHeight)));
  const fps = Math.max(1, Math.min(30, Math.trunc(maxFps)));
  let mask = supportsMjpegCamera() ? VIDEO_CODEC_MJPEG : 0;
  const codecs = [1, 2, 3, 4] as const;
  // Probe sequentially: some hardware exposes fewer simultaneous encoder
  // sessions than formats, and a parallel capability test would create false
  // negatives by competing with itself.
  for (const codec of codecs) {
    if (await probeCameraCodec(codec, width, height, fps)) {
      mask |= cameraCodecBit(codec);
    }
  }
  return mask;
}

/** One line per camera codec, for a log or a bug report: what the browser said
 *  when asked to encode it. */
export function cameraCodecProbeReport(): string {
  const outcomes = cameraCodecProbeOutcomes();
  if (!outcomes.size) return "camera codecs: not probed yet";
  return `camera codecs: ${[...outcomes]
    .map(([codec, outcome]) => `${cameraCodecLabel(codec)}=${outcome}`)
    .join(", ")}`;
}

export interface CameraCapture {
  readonly track: MediaStreamTrack;
  start(): Promise<void>;
  stop(stopTrack?: boolean): void;
  requestKeyframe(): void;
  /** Re-aim the encoder at `scale` times its configured bitrate. */
  setBitrateScale(scale: number): void;
}

export class MjpegCameraCapture implements CameraCapture {
  readonly track: MediaStreamTrack;
  readonly #width: number;
  readonly #height: number;
  readonly #fps: number;
  readonly #frame: (jpeg: Uint8Array, captureUs: number) => void;
  readonly #ended: () => void;
  readonly #canEncode: () => boolean;
  readonly #baseQuality: number;
  #quality: number;
  #video: HTMLVideoElement | null = null;
  #canvas: HTMLCanvasElement | null = null;
  #timer: ReturnType<typeof setInterval> | null = null;
  #encoding = false;
  #startedAt = 0;

  constructor(
    track: MediaStreamTrack,
    width: number,
    height: number,
    fps: number,
    frame: (jpeg: Uint8Array, captureUs: number) => void,
    ended: () => void,
    canEncode: () => boolean,
    jpegQuality = 0.8,
  ) {
    this.track = track;
    this.#width = width;
    this.#height = height;
    this.#fps = fps;
    this.#frame = frame;
    this.#ended = ended;
    this.#canEncode = canEncode;
    this.#baseQuality = jpegQuality;
    this.#quality = jpegQuality;
  }

  async start(): Promise<void> {
    if (this.track.kind !== "video" || this.track.readyState !== "live") {
      throw new Error("camera track is not live");
    }
    if (typeof document === "undefined") {
      throw new Error("camera JPEG encoding requires a document canvas");
    }
    const video = document.createElement("video");
    video.muted = true;
    video.playsInline = true;
    video.srcObject = new MediaStream([this.track]);
    const canvas = document.createElement("canvas");
    canvas.width = this.#width;
    canvas.height = this.#height;
    if (!canvas.getContext("2d", { alpha: false })) {
      throw new Error("2D canvas is unavailable");
    }
    this.#video = video;
    this.#canvas = canvas;
    this.track.addEventListener("ended", this.#ended, { once: true });
    await video.play();
    this.#startedAt = monotonicNow();
    this.#timer = setInterval(() => void this.#encode(), 1_000 / this.#fps);
  }

  stop(stopTrack = true): void {
    this.track.removeEventListener("ended", this.#ended);
    if (this.#timer) clearInterval(this.#timer);
    this.#timer = null;
    if (this.#video) {
      this.#video.pause();
      this.#video.srcObject = null;
    }
    this.#video = null;
    this.#canvas = null;
    if (stopTrack) this.track.stop();
  }

  requestKeyframe(): void {
    // Every Motion JPEG image is independently decodable.
  }

  setBitrateScale(scale: number): void {
    // JPEG quality is the only dial here, and it moves with the governor so
    // a congested link sends smaller pictures rather than fewer.
    this.#quality = Math.min(0.95, Math.max(0.3, this.#baseQuality * scale));
  }

  async #encode(): Promise<void> {
    if (this.#encoding || !this.#video || !this.#canvas || !this.#canEncode()) {
      return;
    }
    this.#encoding = true;
    try {
      const context = this.#canvas.getContext("2d", { alpha: false });
      if (
        !context ||
        this.#video.readyState < HTMLMediaElement.HAVE_CURRENT_DATA
      )
        return;
      context.drawImage(this.#video, 0, 0, this.#width, this.#height);
      const blob = await new Promise<Blob | null>((resolve) =>
        this.#canvas!.toBlob(resolve, "image/jpeg", this.#quality),
      );
      if (!blob || blob.size > 4 * 1024 * 1024) return;
      this.#frame(
        new Uint8Array(await blob.arrayBuffer()),
        Math.round((monotonicNow() - this.#startedAt) * 1_000),
      );
    } finally {
      this.#encoding = false;
    }
  }
}

export class WebCodecsCameraCapture implements CameraCapture {
  readonly track: MediaStreamTrack;
  readonly #codec: Exclude<CameraWireCodec, 0>;
  readonly #width: number;
  readonly #height: number;
  readonly #fps: number;
  readonly #frame: (
    data: Uint8Array,
    captureUs: number,
    keyframe: boolean,
  ) => void;
  readonly #dropped: () => void;
  readonly #ended: () => void;
  readonly #canEncode: () => boolean;
  readonly #failed: (error: Error) => void;
  readonly #baseScale: number;
  #appliedScale: number;
  readonly #encoder: VideoEncoder;
  #video: HTMLVideoElement | null = null;
  /** Target-sized scratch the element is drawn into before encoding. */
  #canvas: HTMLCanvasElement | null = null;
  #timer: ReturnType<typeof setInterval> | null = null;
  #startedAt = 0;
  #lastKeyframeUs = -CAMERA_KEYFRAME_INTERVAL_US;
  #forceKeyframe = true;
  #stopped = false;
  #keyframeHeader: Uint8Array | null = null;

  constructor(
    track: MediaStreamTrack,
    codec: Exclude<CameraWireCodec, 0>,
    width: number,
    height: number,
    fps: number,
    frame: (data: Uint8Array, captureUs: number, keyframe: boolean) => void,
    dropped: () => void,
    ended: () => void,
    canEncode: () => boolean,
    failed: (error: Error) => void,
    bitrateScale = 1,
  ) {
    this.track = track;
    this.#codec = codec;
    this.#width = width;
    this.#height = height;
    this.#fps = fps;
    this.#frame = frame;
    this.#dropped = dropped;
    this.#ended = ended;
    this.#canEncode = canEncode;
    this.#failed = failed;
    this.#encoder = new VideoEncoder({
      output: (chunk) => this.#output(chunk),
      error: (error) => {
        if (!this.#stopped) this.#failed(error);
      },
    });
    this.#baseScale = bitrateScale;
    this.#appliedScale = bitrateScale;
    this.#encoder.configure(
      cameraEncoderConfig(codec, width, height, fps, bitrateScale),
    );
  }

  async start(): Promise<void> {
    if (this.track.kind !== "video" || this.track.readyState !== "live") {
      throw new Error("camera track is not live");
    }
    if (typeof document === "undefined") {
      throw new Error(
        "camera video encoding requires a document video element",
      );
    }
    const video = document.createElement("video");
    video.muted = true;
    video.playsInline = true;
    video.srcObject = new MediaStream([this.track]);
    this.#video = video;
    this.track.addEventListener("ended", this.#ended, { once: true });
    await video.play();
    if (this.#stopped || this.track.readyState !== "live") {
      throw new Error("camera track ended during initialization");
    }
    const canvas = document.createElement("canvas");
    canvas.width = this.#width;
    canvas.height = this.#height;
    if (!canvas.getContext("2d", { alpha: false })) {
      throw new Error("2D canvas is unavailable");
    }
    this.#canvas = canvas;
    this.#startedAt = monotonicNow();
    this.#timer = setInterval(() => this.#encode(), 1_000 / this.#fps);
  }

  stop(stopTrack = true): void {
    if (this.#stopped) {
      if (stopTrack && this.track.readyState === "live") this.track.stop();
      return;
    }
    this.#stopped = true;
    this.track.removeEventListener("ended", this.#ended);
    if (this.#timer) clearInterval(this.#timer);
    this.#timer = null;
    if (this.#video) {
      this.#video.pause();
      this.#video.srcObject = null;
    }
    this.#canvas = null;
    this.#video = null;
    try {
      this.#encoder.close();
    } catch {
      // WebCodecs may already have closed after its error callback.
    }
    if (stopTrack) this.track.stop();
  }

  requestKeyframe(): void {
    this.#forceKeyframe = true;
  }

  setBitrateScale(scale: number): void {
    const next = this.#baseScale * scale;
    // Reconfiguring costs the encoder its reference state, so ignore the
    // noise and act on real moves only.
    if (Math.abs(next - this.#appliedScale) < this.#appliedScale * 0.1) return;
    this.#appliedScale = next;
    try {
      this.#encoder.configure(
        cameraEncoderConfig(
          this.#codec,
          this.#width,
          this.#height,
          this.#fps,
          next,
        ),
      );
      // A reconfigured encoder starts a new sequence; the decoder on the far
      // side needs a keyframe to follow it.
      this.#forceKeyframe = true;
    } catch {
      // An encoder that refuses the new bitrate keeps the old one, which is
      // survivable — the frame drop path still bounds the delay.
    }
  }

  #encode(): void {
    if (
      this.#stopped ||
      !this.#video ||
      !this.#canEncode() ||
      this.#video.readyState < HTMLMediaElement.HAVE_CURRENT_DATA
    ) {
      return;
    }
    if (this.#encoder.encodeQueueSize >= 2) {
      this.#dropped();
      return;
    }
    const captureUs = Math.max(
      0,
      Math.round((monotonicNow() - this.#startedAt) * 1_000),
    );
    const keyframe =
      this.#forceKeyframe ||
      captureUs - this.#lastKeyframeUs >= CAMERA_KEYFRAME_INTERVAL_US;
    let frame: VideoFrame | null = null;
    try {
      // Via a canvas, not straight off the element.
      //
      // `new VideoFrame(video)` takes the frame as decoded and ignores the
      // rotation the element applies when it paints, so a tablet whose camera
      // is mounted against the way it is held encodes upside down while its
      // own preview — and Motion JPEG, which has always gone through
      // `drawImage` — look right. Drawing first puts both codecs on the one
      // path that honours it, and costs a copy the JPEG path already paid.
      const context = this.#canvas?.getContext("2d", { alpha: false });
      if (!context || !this.#canvas) return;
      context.drawImage(this.#video, 0, 0, this.#width, this.#height);
      frame = new VideoFrame(this.#canvas, { timestamp: captureUs });
      this.#encoder.encode(frame, { keyFrame: keyframe });
      if (keyframe) {
        this.#forceKeyframe = false;
        this.#lastKeyframeUs = captureUs;
      }
    } catch (error) {
      this.#failed(error instanceof Error ? error : new Error(String(error)));
    } finally {
      frame?.close();
    }
  }

  #output(chunk: EncodedVideoChunk): void {
    if (this.#stopped) return;
    if (chunk.byteLength === 0 || chunk.byteLength > CAMERA_FRAME_MAX) {
      this.#forceKeyframe = true;
      this.#dropped();
      return;
    }
    let data: Uint8Array = new Uint8Array(chunk.byteLength);
    chunk.copyTo(data);
    if (chunk.type === "key") {
      const decision = cameraKeyframeForWire(
        this.#codec,
        data,
        this.#keyframeHeader,
      );
      if (decision.action === "reject") {
        this.#failed(
          new Error(
            `${cameraCodecLabel(this.#codec)} encoder emitted the wrong chroma format`,
          ),
        );
        return;
      }
      if (decision.action === "drop") {
        this.#forceKeyframe = true;
        this.#dropped();
        return;
      }
      data = decision.data;
      if (decision.header) this.#keyframeHeader = decision.header;
    }
    this.#frame(data, chunk.timestamp, chunk.type === "key");
  }
}

/** Browser presentation state and controls for the native YAS Media family. */
export class MediaStore implements ReactiveStore {
  readonly #notifier = new Notifier();
  readonly #requests = new Map<MediaId, PortalRequest>();
  readonly #requestListeners = new Set<(request: PortalRequest) => void>();
  #native: NativeMediaController | null = null;
  #state: DesktopMediaState = emptyState();
  #microphone: MediaLeaseState = emptyLease("microphone");
  #camera: MediaLeaseState = emptyLease("camera");
  #cameraTrack: MediaStreamTrack | null = null;

  get revision(): number {
    return this.#notifier.revision;
  }

  get state(): DesktopMediaState {
    return this.#state;
  }

  get serverVideoCodecs(): number {
    return VIDEO_CODECS_ALL;
  }

  get microphone(): MediaLeaseState {
    return this.#microphone;
  }

  get camera(): MediaLeaseState {
    return this.#camera;
  }

  get cameraTrack(): MediaStreamTrack | null {
    return this.#cameraTrack;
  }

  get requests(): ReadonlyMap<MediaId, PortalRequest> {
    return this.#requests;
  }

  subscribe(listener: () => void): () => void {
    return this.#notifier.subscribe(listener);
  }

  setNativeController(controller: NativeMediaController | null): void {
    this.#native = controller;
  }

  replaceNativeState(state: DesktopMediaState): void {
    this.#state = state;
    this.#notifier.emit();
  }

  publishNativeRequest(request: PortalRequest): void {
    this.#requests.set(request.requestId, request);
    this.#notifier.emit();
    for (const listener of [...this.#requestListeners]) listener(request);
  }

  removeNativeRequest(requestId: MediaId): void {
    if (this.#requests.delete(requestId)) this.#notifier.emit();
  }

  publishNativeLease(
    kind: "microphone" | "camera",
    lease: MediaLeaseState,
  ): void {
    if (kind === "microphone") this.#microphone = lease;
    else this.#camera = lease;
    this.#notifier.emit();
  }

  publishNativeCameraTrack(track: MediaStreamTrack | null): void {
    this.#cameraTrack = track;
    this.#notifier.emit();
  }

  advertise(_capabilities: MediaCapabilities): void {}

  setCapabilities(capabilities: MediaCapabilities): void {
    this.advertise(capabilities);
  }

  async startMicrophone(
    track: MediaStreamTrack,
    options: MicrophoneOptions = {},
  ): Promise<void> {
    if (!this.#native) {
      track.stop();
      throw new Error("Media family unavailable");
    }
    await this.#native.startMicrophone(track, options);
  }

  async startCamera(
    track: MediaStreamTrack,
    options: CameraOptions = {},
  ): Promise<void> {
    if (!this.#native) {
      track.stop();
      throw new Error("Media family unavailable");
    }
    await this.#native.startCamera(track, options);
  }

  stop(kind: "microphone" | "camera"): void {
    this.#native?.stop(kind);
  }

  onPortalRequest(listener: (request: PortalRequest) => void): () => void {
    this.#requestListeners.add(listener);
    return () => this.#requestListeners.delete(listener);
  }

  reply(
    requestId: MediaId,
    decision: "deny" | "grant" | "cancelled",
    surfaceIds: readonly SurfaceId[] = [],
    choices: readonly PortalChoiceValue[] = [],
  ): void {
    const request = this.#requests.get(requestId);
    if (!request || !this.#native) return;
    void this.#native
      .reply(request, decision, surfaceIds, choices)
      .then(() => this.removeNativeRequest(requestId));
  }

  stopScreenCast(sessionId: MediaId): void {
    if (!this.#native || typeof sessionId !== "bigint" || sessionId === 0n)
      return;
    void this.#native.stopScreenCast(sessionId);
  }

  reset(): void {
    const changed =
      this.#state.runtimeFlags !== 0 ||
      this.#state.activeFlags !== 0 ||
      this.#requests.size > 0 ||
      this.#microphone.status !== "inactive" ||
      this.#camera.status !== "inactive" ||
      this.#cameraTrack !== null;
    this.#state = emptyState();
    this.#requests.clear();
    this.#microphone = emptyLease("microphone");
    this.#camera = emptyLease("camera");
    this.#cameraTrack = null;
    if (changed) this.#notifier.emit();
  }
}

function emptyState(): DesktopMediaState {
  return {
    runtimeFlags: 0,
    activeFlags: 0,
    microphoneOwner: 0n,
    cameraOwner: 0n,
    screencasts: [],
  };
}

function emptyLease(kind: "microphone" | "camera"): MediaLeaseState {
  return {
    kind,
    status: "inactive",
    leaseId: 0n,
    codec: 0,
    width: 0,
    height: 0,
    fps: 0,
    credit: 0,
    error: null,
  };
}

function monotonicNow(): number {
  return typeof performance !== "undefined" ? performance.now() : Date.now();
}
