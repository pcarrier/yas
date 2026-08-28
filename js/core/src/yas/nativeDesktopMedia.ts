import {
  MENU_NODE_CHECKMARK,
  MENU_NODE_ENABLED,
  MENU_NODE_RADIO,
  MENU_NODE_SEPARATOR,
  MENU_NODE_SUBMENU,
  MENU_NODE_VISIBLE,
  NOTIFICATION_RESIDENT,
  NOTIFICATION_TRANSIENT,
  TRAY_HAS_MENU,
  DesktopStore,
  type DesktopId,
  type DesktopImage,
  type DesktopNotification,
  type DesktopRevision,
  type NativeDesktopController,
  type TrayItem,
} from "../desktopModel";
import { AudioPlayer } from "../AudioPlayer";
import {
  ACTIVE_CAMERA,
  ACTIVE_MICROPHONE,
  ACTIVE_SCREENCAST,
  RUNTIME_CAMERA,
  RUNTIME_MICROPHONE,
  RUNTIME_MPRIS,
  RUNTIME_PORTAL_ACCESS,
  RUNTIME_PORTAL_FRONTEND,
  RUNTIME_PORTAL_SCREENCAST,
  MediaStore,
  MprisStore,
  MjpegCameraCapture,
  OpusMicrophoneEncoder,
  PcmMicrophoneCapture,
  WebCodecsCameraCapture,
  cameraCodecCandidates,
  cameraQuality,
  probeCameraCodec,
  supportsMjpegCamera,
  supportsOpusMicrophone,
  type CameraCapture,
  type CameraOptions,
  type CameraWireCodec,
  type DesktopMediaState,
  type MediaLeaseState,
  type MicrophoneOptions,
  type MprisAction,
  type MprisPlayer,
  type NativeMediaController,
  type NativeMprisController,
  type PortalRequest,
  type PortalChoiceValue,
} from "../mediaModel";
import type {
  YasClientAuxSubscription,
  YasClientInfo,
  YasClientList,
  YasClientOrigin as ProductClientOrigin,
} from "../types";
import * as g from "./generated";
import {
  YasClientClient,
  type YasClientOrigin,
  type YasClientRecord,
  type YasClientSnapshot,
} from "./client";
import {
  YasDesktopClient,
  type YasDesktopNotificationRecord,
  type YasDesktopSnapshot,
  type YasDesktopTrayRecord,
} from "./desktop";
import {
  YasMediaClient,
  mediaPlayerActive,
  mediaPlayerAlbumArtUrl,
  type YasMediaDeviceRecord,
  type YasMediaFormat,
  type YasMediaFrame,
  type YasMediaFrameAck,
  type YasMediaPlayerRecord,
  type YasMediaPortalRequest,
  type YasMediaSnapshot,
  type YasMediaStreamStatus,
} from "./media";
import type { YasConnection } from "./session";
import { YasCursor, YasProtocolError, YasResultError, YasWriter } from "./wire";

const EMPTY_IMAGE: DesktopImage = {
  width: 0,
  height: 0,
  png: new Uint8Array(),
};
const MAX_ASSET_ITEMS = 256;
const MAX_ASSET_BYTES = 64 * 1024 * 1024;
const MAX_ASSET_SINGLE_BYTES = 8 * 1024 * 1024;
const MAX_IMAGE_DIMENSION = 8_192;
const MAX_IMAGE_PIXELS = 16 * 1024 * 1024;
const MAX_PLAYER_ARTWORK_ITEMS = 128;
const MAX_PLAYER_ARTWORK_BYTES = 64 * 1024 * 1024;
const MAX_PLAYER_ARTWORK_SINGLE_BYTES = 512 * 1024;
const MEDIA_CAPTURE_LEASE_NS = 60n * 60n * 1_000_000_000n;
const MEDIA_FRAME_FRAGMENT_BYTES = 64 * 1024;
const MEDIA_FRAME_FRAGMENT_MAX = 64;
const MEDIA_AUDIO_FRAGMENT_MAX = 16;
const MEDIA_AUDIO_RETAINED_MAX = 16 * 1024 * 1024;
const MEDIA_OUTPUT_CREDIT = 64;

interface NativeInputStream {
  kind: "microphone" | "camera";
  leaseHandle: bigint;
  streamHandle: bigint;
  format: YasMediaFormat;
  sequence: bigint;
  creditFrames: number;
  discontinuity: boolean;
  keyframeRequired: boolean;
}

interface NativeAudioReassembly {
  sequence: bigint;
  captureTime: bigint;
  presentationTime: bigint;
  codecVersion: number;
  flags: number;
  fragmentCount: number;
  nextFragment: number;
  completeLength: number;
  bytes: Uint8Array;
  received: number;
  lease: { release(): void };
}

interface NativeAudioOutput {
  streamHandle: bigint;
  bitrateKbps: number;
  sampleRate: number;
  consumedSequence: bigint;
  reassembly: NativeAudioReassembly | null;
}

export interface YasNativeDesktopClientLifecycleOptions {
  session: YasConnection;
  desktopStore: DesktopStore;
  mediaStore: MediaStore;
  mprisStore: MprisStore;
  audioPlayer: AudioPlayer;
  onChanged?: () => void;
  onError?: (error: Error) => void;
}

/** Direct typed Desktop, Client, Media, MPRIS, and audio lifecycle.
 *
 * It adapts decoded family records to UI-domain records while retaining native
 * resource handles. */
export class YasNativeDesktopClientLifecycle {
  readonly supportsDesktop: boolean;
  readonly supportsClientControl: boolean;
  readonly supportsDesktopMedia: boolean;
  readonly supportsAudio: boolean;

  private readonly desktop: YasDesktopClient | null;
  private readonly client: YasClientClient | null;
  private readonly media: YasMediaClient | null;
  private removeDesktop: (() => void) | null = null;
  private removeClient: (() => void) | null = null;
  private removeMedia: (() => void) | null = null;
  private removePortal: (() => void) | null = null;
  private removeFrame: (() => void) | null = null;
  private removeFrameAck: (() => void) | null = null;
  private removeStreamStatus: (() => void) | null = null;
  private readonly clientListeners = new Set<{
    listener: (catalog: YasClientList) => void;
    onError?: (error: Error) => void;
  }>();
  private clientSnapshot: YasClientList | null = null;
  private mediaSnapshot: YasMediaSnapshot | null = null;
  private pendingMedia: YasMediaSnapshot | null = null;
  private mediaDrain: Promise<void> | null = null;
  private mediaGeneration = 0;
  private readonly portalRequests = new Map<bigint, YasMediaPortalRequest>();
  private microphone: NativeInputStream | null = null;
  private camera: NativeInputStream | null = null;
  private microphoneCapture: PcmMicrophoneCapture | null = null;
  private microphoneEncoder: OpusMicrophoneEncoder | null = null;
  private cameraCapture: CameraCapture | null = null;
  private cameraPendingTrack: MediaStreamTrack | null = null;
  private microphoneGeneration = 0;
  private cameraGeneration = 0;
  private audioOutput: NativeAudioOutput | null = null;
  private audioOutputGeneration = 0;
  private pendingAudioOperation: (() => void | Promise<void>) | null = null;
  private audioOutputDrain: Promise<void> | null = null;
  private desiredAudioBitrate: number | null = null;
  private desktopInitial = true;
  private desktopGeneration = 0;
  private pendingDesktop: YasDesktopSnapshot | null = null;
  private desktopDrain: Promise<void> | null = null;
  private disposed = false;
  private readonly assets = new Map<
    string,
    { image: DesktopImage; bytes: number }
  >();
  private assetBytes = 0;
  private readonly playerArtwork = new Map<
    string,
    { artwork: NonNullable<MprisPlayer["artwork"]>; bytes: number }
  >();
  private playerArtworkBytes = 0;

  constructor(
    private readonly options: YasNativeDesktopClientLifecycleOptions,
  ) {
    this.supportsDesktop = operationsAvailable(
      options.session,
      g.YAS_FAMILY_DESKTOP,
      [
        [g.YAS_CLASS_REQUEST, g.YAS_DESKTOP_WATCH],
        [g.YAS_CLASS_REQUEST, g.YAS_DESKTOP_UNWATCH],
        [g.YAS_CLASS_REQUEST, g.YAS_DESKTOP_GET_MENU],
        [g.YAS_CLASS_REQUEST, g.YAS_DESKTOP_TRAY_ACTION],
        [g.YAS_CLASS_REQUEST, g.YAS_DESKTOP_NOTIFICATION_ACTION],
        [g.YAS_CLASS_REQUEST, g.YAS_DESKTOP_FETCH_ASSET],
        [g.YAS_CLASS_EVENT, g.YAS_DESKTOP_STATE, true],
        [g.YAS_CLASS_EVENT, g.YAS_DESKTOP_STATE_ACK],
      ],
    );
    this.supportsClientControl = operationsAvailable(
      options.session,
      g.YAS_FAMILY_CLIENT,
      [
        [g.YAS_CLASS_REQUEST, g.YAS_CLIENT_WATCH],
        [g.YAS_CLASS_REQUEST, g.YAS_CLIENT_UNWATCH],
        [g.YAS_CLASS_REQUEST, g.YAS_CLIENT_DISCONNECT],
        [g.YAS_CLASS_EVENT, g.YAS_CLIENT_STATE, true],
        [g.YAS_CLASS_EVENT, g.YAS_CLIENT_STATE_ACK],
      ],
    );
    const mediaCatalogue = operationsAvailable(
      options.session,
      g.YAS_FAMILY_MEDIA,
      [
        [g.YAS_CLASS_REQUEST, g.YAS_MEDIA_WATCH],
        [g.YAS_CLASS_REQUEST, g.YAS_MEDIA_UNWATCH],
        [g.YAS_CLASS_EVENT, g.YAS_MEDIA_STATE, true],
        [g.YAS_CLASS_EVENT, g.YAS_MEDIA_STATE_ACK],
      ],
    );
    this.supportsDesktopMedia =
      mediaCatalogue &&
      operationsAvailable(options.session, g.YAS_FAMILY_MEDIA, [
        [g.YAS_CLASS_REQUEST, g.YAS_MEDIA_ACQUIRE_DEVICE],
        [g.YAS_CLASS_REQUEST, g.YAS_MEDIA_RELEASE_DEVICE],
        [g.YAS_CLASS_REQUEST, g.YAS_MEDIA_PORTAL_REPLY],
        [g.YAS_CLASS_REQUEST, g.YAS_MEDIA_PLAYER_ACTION],
        [g.YAS_CLASS_REQUEST, g.YAS_MEDIA_CLOSE_STREAM],
        [g.YAS_CLASS_REQUEST, g.YAS_MEDIA_FETCH_ASSET],
        [g.YAS_CLASS_REQUEST, g.YAS_MEDIA_PORTAL_CLOSE],
        [g.YAS_CLASS_EVENT, g.YAS_MEDIA_PORTAL_REQUEST, true],
        [g.YAS_CLASS_EVENT, g.YAS_MEDIA_FRAME],
        [g.YAS_CLASS_EVENT, g.YAS_MEDIA_FRAME, true],
        [g.YAS_CLASS_EVENT, g.YAS_MEDIA_FRAME_ACK],
        [g.YAS_CLASS_EVENT, g.YAS_MEDIA_FRAME_ACK, true],
        [g.YAS_CLASS_EVENT, g.YAS_MEDIA_STREAM_STATUS, true],
      ]);
    this.supportsAudio =
      mediaCatalogue &&
      operationsAvailable(options.session, g.YAS_FAMILY_MEDIA, [
        [g.YAS_CLASS_REQUEST, g.YAS_MEDIA_OPEN_OUTPUT],
        [g.YAS_CLASS_REQUEST, g.YAS_MEDIA_CLOSE_STREAM],
        [g.YAS_CLASS_EVENT, g.YAS_MEDIA_FRAME, true],
        [g.YAS_CLASS_EVENT, g.YAS_MEDIA_FRAME_ACK],
        [g.YAS_CLASS_EVENT, g.YAS_MEDIA_STREAM_STATUS, true],
      ]);
    this.desktop = this.supportsDesktop
      ? new YasDesktopClient(options.session)
      : null;
    this.client = this.supportsClientControl
      ? new YasClientClient(options.session)
      : null;
    this.media = mediaCatalogue ? new YasMediaClient(options.session) : null;
    options.desktopStore.setNativeController(
      this.desktop ? this.desktopController() : null,
    );
    options.mediaStore.setNativeController(
      this.media ? this.mediaController() : null,
    );
    options.mprisStore.setNativeController(
      this.media ? this.mprisController() : null,
    );
  }

  async start(): Promise<void> {
    if (this.disposed) return;
    const tasks: Promise<void>[] = [];
    if (this.desktop) {
      this.removeDesktop = this.desktop.catalog.subscribe((snapshot) => {
        this.queueDesktop(snapshot);
      });
      tasks.push(this.watchWhileLive(this.desktop.catalog));
    }
    if (this.client) {
      this.removeClient = this.client.catalog.subscribe((snapshot) => {
        this.publishClients(snapshot);
      });
      tasks.push(this.watchWhileLive(this.client.catalog));
    }
    if (this.media) {
      this.removeMedia = this.media.catalog.subscribe((snapshot) => {
        this.queueMedia(snapshot);
      });
      this.removePortal = this.media.onPortalRequest((request) => {
        void this.publishPortalRequest(request).catch((error: unknown) =>
          this.report(error),
        );
      });
      this.removeFrame = this.media.onFrame((frame) => {
        try {
          this.handleMediaFrame(frame);
        } catch (error) {
          this.report(error);
          this.sendAudioUnsubscribe();
        }
      });
      this.removeFrameAck = this.media.onFrameAck((ack) =>
        this.handleMediaFrameAck(ack),
      );
      this.removeStreamStatus = this.media.onStreamStatus((status) =>
        this.handleMediaStreamStatus(status),
      );
      tasks.push(this.watchWhileLive(this.media.catalog));
    }
    try {
      await Promise.all(tasks);
    } catch (error) {
      this.dispose();
      throw error;
    }
  }

  private async watchWhileLive(catalog: {
    watch(): Promise<void>;
    unwatch(): Promise<void>;
  }): Promise<void> {
    await catalog.watch();
    // dispose() can run while WATCH is in flight, before the catalogue owns a
    // subscription. Clean it again once the request has actually settled.
    if (this.disposed) await catalog.unwatch().catch(() => undefined);
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.desktopGeneration++;
    this.mediaGeneration++;
    this.removeDesktop?.();
    this.removeClient?.();
    this.removeMedia?.();
    this.removePortal?.();
    this.removeFrame?.();
    this.removeFrameAck?.();
    this.removeStreamStatus?.();
    this.removeDesktop = null;
    this.removeClient = null;
    this.removeMedia = null;
    this.removePortal = null;
    this.removeFrame = null;
    this.removeFrameAck = null;
    this.removeStreamStatus = null;
    // Disposal is also reached from session invalidation, after the transport
    // can no longer carry UNWATCH. Local teardown is authoritative here; the
    // wire cleanup is best-effort and must not become an unhandled rejection.
    void this.desktop?.catalog.unwatch().catch(() => undefined);
    void this.client?.catalog.unwatch().catch(() => undefined);
    void this.media?.catalog.unwatch().catch(() => undefined);
    this.desktop?.dispose();
    this.client?.dispose();
    this.media?.dispose();
    this.stopCapture("microphone");
    this.stopCapture("camera");
    this.sendAudioUnsubscribe();
    this.options.desktopStore.setNativeController(null);
    this.options.desktopStore.reset();
    this.options.mediaStore.setNativeController(null);
    this.options.mediaStore.reset();
    this.options.mprisStore.setNativeController(null);
    this.options.mprisStore.reset();
    this.options.audioPlayer.reset();
    this.clientListeners.clear();
    this.clientSnapshot = null;
    this.assets.clear();
    this.assetBytes = 0;
    this.playerArtwork.clear();
    this.playerArtworkBytes = 0;
  }

  subscribeClients(
    listener: (catalog: YasClientList) => void,
    onError?: (error: Error) => void,
  ): () => void {
    if (!this.client) {
      queueMicrotask(() => onError?.(new Error("Client family unavailable")));
      return () => undefined;
    }
    const entry = { listener, onError };
    this.clientListeners.add(entry);
    if (this.clientSnapshot) listener(this.clientSnapshot);
    return () => this.clientListeners.delete(entry);
  }

  async listClients(): Promise<YasClientList> {
    if (!this.client) throw new Error("Client family unavailable");
    if (!this.clientSnapshot) this.publishClients(await this.client.list());
    return this.clientSnapshot!;
  }

  async kickClient(id: string, reason: string): Promise<void> {
    if (!this.client) throw new Error("Client family unavailable");
    const sessionId = decodeSessionId(id);
    await this.client.disconnect(sessionId, operationId(), reason);
  }

  sendAudioSubscribe(bitrateKbps = 0): void {
    if (!this.media) return;
    if (
      !Number.isInteger(bitrateKbps) ||
      bitrateKbps < 0 ||
      bitrateKbps > g.YAS_MEDIA_MAX_OUTPUT_BITRATE_KBPS
    ) {
      this.report(new Error("invalid Media output bitrate"));
      return;
    }
    this.desiredAudioBitrate = bitrateKbps;
    this.queueAudio(async () => {
      if (this.audioOutput?.bitrateKbps === bitrateKbps) return;
      await this.closeAudioOutput();
      const device = this.mediaSnapshot?.devices.find(
        (candidate) =>
          candidate.deviceKind === g.YAS_MEDIA_KIND_AUDIO_OUTPUT &&
          candidate.state === g.YAS_MEDIA_DEVICE_AVAILABLE &&
          candidate.formats.some(
            (format) =>
              format.codec === g.YAS_MEDIA_CODEC_OPUS &&
              format.sampleRate === Number(g.YAS_MEDIA_WIRE_SAMPLE_RATE) &&
              format.channels === 2,
          ),
      );
      const format = device?.formats.find(
        (candidate) =>
          candidate.codec === g.YAS_MEDIA_CODEC_OPUS &&
          candidate.sampleRate === Number(g.YAS_MEDIA_WIRE_SAMPLE_RATE) &&
          candidate.channels === 2,
      );
      if (!device || !format)
        throw new Error("Media audio output is unavailable");
      const generation = this.audioOutputGeneration;
      const result = await this.media!.openOutput({
        deviceHandle: device.deviceHandle,
        formats: [format],
        latencyTargetNs: 60_000_000n,
        targetBitrateKbps: bitrateKbps,
      });
      if (!mediaFormatsEqual(result.selectedFormat, format)) {
        await this.media!.closeStream(result.streamHandle, operationId());
        throw new Error("Media output selected an unoffered format");
      }
      if (generation !== this.audioOutputGeneration || this.disposed) {
        await this.media!.closeStream(result.streamHandle, operationId());
        return;
      }
      this.audioOutput = {
        streamHandle: result.streamHandle,
        bitrateKbps,
        sampleRate: result.selectedFormat.sampleRate,
        consumedSequence: 0n,
        reassembly: null,
      };
      this.media!.sendFrameAck({
        streamHandle: result.streamHandle,
        consumedSequence: 0n,
        queueDepth: 0,
        desiredCreditFrames: MEDIA_OUTPUT_CREDIT,
      });
    });
  }

  sendAudioUnsubscribe(): void {
    this.desiredAudioBitrate = null;
    this.audioOutputGeneration++;
    this.queueAudio(() => this.closeAudioOutput());
  }

  private mediaController(): NativeMediaController {
    return {
      startMicrophone: (track, options) => this.startMicrophone(track, options),
      startCamera: (track, options) => this.startCamera(track, options),
      stop: (kind) => this.stopCapture(kind),
      reply: (request, decision, surfaceIds, choices) =>
        this.reported(() =>
          this.replyPortal(request, decision, surfaceIds, choices),
        ),
      stopScreenCast: (sessionId) =>
        this.reported(() => this.stopScreenCast(sessionId)),
    };
  }

  private mprisController(): NativeMprisController {
    return {
      subscribe: () => undefined,
      act: async (playerId, action) => {
        if (!this.media) throw new Error("Media family unavailable");
        const mapped = nativeMprisAction(action);
        if (mapped === null) return;
        await this.media.playerAction({
          playerHandle: playerId,
          operationId: operationId(),
          action: mapped.action,
          value: mapped.value,
        });
      },
    };
  }

  private async replyPortal(
    request: PortalRequest,
    decision: "deny" | "grant" | "cancelled",
    surfaceIds: readonly bigint[],
    choices: readonly PortalChoiceValue[],
  ): Promise<void> {
    if (!this.media || typeof request.requestId !== "bigint")
      throw new Error("Media portal is unavailable");
    const native = this.portalRequests.get(request.requestId);
    if (!native) throw new Error("Media portal request no longer exists");
    const grant = decision === "grant";
    const metadata = !grant
      ? ({ kind: "empty" } as const)
      : native.metadata.kind === "access"
        ? ({ kind: "accessGrant", choices } as const)
        : ({ kind: "screencastGrant", surfaceHandles: surfaceIds } as const);
    await this.media.portalReply({
      portalHandle: native.portalHandle,
      revision: native.revision,
      operationId: operationId(),
      kind: native.kind,
      decision:
        decision === "grant"
          ? g.YAS_MEDIA_PORTAL_DECISION_GRANT
          : decision === "cancelled"
            ? g.YAS_MEDIA_PORTAL_DECISION_CANCEL
            : g.YAS_MEDIA_PORTAL_DECISION_DENY,
      metadata,
    });
    this.portalRequests.delete(native.portalHandle);
  }

  private async stopScreenCast(portalHandle: bigint): Promise<void> {
    if (!this.media) throw new Error("Media family unavailable");
    const record = this.mediaSnapshot?.portals.find(
      (candidate) => candidate.portalHandle === portalHandle,
    );
    if (!record) return;
    await this.media.portalClose({
      portalHandle,
      revision: record.revision,
      operationId: operationId(),
    });
  }

  private async startMicrophone(
    track: MediaStreamTrack,
    options: MicrophoneOptions,
  ): Promise<void> {
    if (!this.media || !this.mediaSnapshot) {
      track.stop();
      throw new Error("Media family is not ready");
    }
    if (this.microphone || this.microphoneCapture) {
      track.stop();
      throw new Error("microphone capture is already active");
    }
    const generation = ++this.microphoneGeneration;
    const preferOpus =
      options.codec === "opus" ||
      (options.codec === undefined && supportsOpusMicrophone());
    const codecs = preferOpus
      ? [g.YAS_MEDIA_CODEC_OPUS, g.YAS_MEDIA_CODEC_PCM_S16LE]
      : [g.YAS_MEDIA_CODEC_PCM_S16LE];
    const selected = findDeviceFormat(
      this.mediaSnapshot.devices,
      g.YAS_MEDIA_KIND_MICROPHONE,
      codecs,
      (format) => format.channels === 1 && format.sampleRate === 48_000,
    );
    if (!selected) {
      track.stop();
      throw new Error("no compatible native microphone device");
    }
    if (
      options.codec === "opus" &&
      selected.format.codec !== g.YAS_MEDIA_CODEC_OPUS
    ) {
      track.stop();
      throw new Error("native microphone does not support Opus");
    }
    this.options.mediaStore.publishNativeLease("microphone", {
      ...emptyNativeLease("microphone"),
      status: "starting",
    });
    const capture = new PcmMicrophoneCapture(
      track,
      (pcm, captureUs) => {
        if (
          generation !== this.microphoneGeneration ||
          this.microphoneCapture !== capture
        )
          return;
        if (this.microphoneEncoder)
          this.microphoneEncoder.encode(pcm, captureUs);
        else this.sendInputFrame("microphone", pcm, captureUs, false);
      },
      () => {
        if (
          generation === this.microphoneGeneration &&
          this.microphoneCapture === capture
        )
          this.stopCapture("microphone", "microphone device ended", false);
      },
    );
    this.microphoneCapture = capture;
    let encoder: OpusMicrophoneEncoder | null = null;
    try {
      await capture.start();
      if (
        generation !== this.microphoneGeneration ||
        this.microphoneCapture !== capture
      )
        throw new Error("microphone capture was cancelled");
      if (selected.format.codec === g.YAS_MEDIA_CODEC_OPUS) {
        encoder = await OpusMicrophoneEncoder.create(
          (packet, captureUs) =>
            this.sendInputFrame(
              "microphone",
              encodeOpusPackets([packet]),
              captureUs,
              false,
            ),
          (error) => {
            if (generation === this.microphoneGeneration)
              this.stopCapture("microphone", error.message);
          },
        );
        if (
          generation !== this.microphoneGeneration ||
          this.microphoneCapture !== capture
        ) {
          encoder.stop();
          throw new Error("microphone capture was cancelled");
        }
        this.microphoneEncoder = encoder;
      }
      const result = await this.media.acquireDevice({
        deviceHandle: selected.device.deviceHandle,
        operationId: operationId(),
        kind: g.YAS_MEDIA_KIND_MICROPHONE,
        leaseDurationNs: MEDIA_CAPTURE_LEASE_NS,
        formats: [selected.format],
      });
      if (
        generation !== this.microphoneGeneration ||
        this.microphoneCapture !== capture
      ) {
        await Promise.allSettled([
          this.media.releaseDevice(result.leaseHandle, operationId()),
          this.media.closeStream(result.streamHandle, operationId()),
        ]);
        throw new Error("microphone capture was cancelled");
      }
      if (!mediaFormatsEqual(result.selectedFormat, selected.format)) {
        await Promise.allSettled([
          this.media.releaseDevice(result.leaseHandle, operationId()),
          this.media.closeStream(result.streamHandle, operationId()),
        ]);
        throw new Error("Media microphone selected an unoffered format");
      }
      this.microphone = {
        kind: "microphone",
        leaseHandle: result.leaseHandle,
        streamHandle: result.streamHandle,
        format: result.selectedFormat,
        sequence: 0n,
        creditFrames: 0,
        discontinuity: false,
        keyframeRequired: false,
      };
      this.options.mediaStore.publishNativeLease("microphone", {
        kind: "microphone",
        status: "active",
        leaseId: result.leaseHandle,
        codec: result.selectedFormat.codec === g.YAS_MEDIA_CODEC_OPUS ? 1 : 0,
        width: 0,
        height: 0,
        fps: 0,
        credit: 0,
        error: null,
      });
    } catch (error) {
      if (generation === this.microphoneGeneration) {
        this.stopCapture(
          "microphone",
          error instanceof Error ? error.message : String(error),
        );
      } else {
        capture.stop();
        encoder?.stop();
      }
      throw error;
    }
  }

  private async startCamera(
    track: MediaStreamTrack,
    options: CameraOptions,
  ): Promise<void> {
    if (!this.media || !this.mediaSnapshot) {
      track.stop();
      throw new Error("Media family is not ready");
    }
    if (this.camera || this.cameraCapture || this.cameraPendingTrack) {
      track.stop();
      throw new Error("camera capture is already active");
    }
    const generation = ++this.cameraGeneration;
    this.cameraPendingTrack = track;
    const settings = track.getSettings();
    const width = evenDimension(options.width ?? settings.width ?? 1280, 1920);
    const height = evenDimension(
      options.height ?? settings.height ?? 720,
      1080,
    );
    const fps = Math.max(
      1,
      Math.min(60, Math.round(options.fps ?? settings.frameRate ?? 30)),
    );
    let selected:
      | {
          device: YasMediaDeviceRecord;
          format: YasMediaFormat;
          wire: CameraWireCodec;
        }
      | undefined;
    for (const wire of cameraCodecCandidates(options)) {
      const codec = nativeCameraCodec(wire);
      const advertised = findDeviceFormat(
        this.mediaSnapshot.devices,
        g.YAS_MEDIA_KIND_CAMERA,
        [codec],
        () => true,
      );
      if (!advertised) continue;
      // A camera catalogue format advertises a codec, not one fixed camera
      // mode. The physical camera belongs to the viewer and is unknown to the
      // server until ACQUIRE_DEVICE; the server deliberately accepts the
      // dimensions and cadence offered there. Requiring them to equal the
      // catalogue's representative 1920x1080@30 entry rejected ordinary Mac
      // camera modes such as 1280x720 before negotiation even began.
      const candidate = {
        device: advertised.device,
        format: cameraCaptureFormat(advertised.format, width, height, fps),
      };
      const supported =
        wire === 0
          ? supportsMjpegCamera()
          : await probeCameraCodec(wire, width, height, fps).catch((error) => {
              if (this.cameraPendingTrack === track)
                this.cameraPendingTrack = null;
              track.stop();
              throw error;
            });
      if (generation !== this.cameraGeneration) {
        track.stop();
        throw new Error("camera capture was cancelled");
      }
      if (supported) {
        selected = { ...candidate, wire };
        break;
      }
    }
    if (!selected) {
      if (this.cameraPendingTrack === track) this.cameraPendingTrack = null;
      track.stop();
      throw new Error("no compatible native camera format");
    }
    this.options.mediaStore.publishNativeCameraTrack(track);
    this.options.mediaStore.publishNativeLease("camera", {
      ...emptyNativeLease("camera"),
      status: "starting",
      width,
      height,
      fps,
    });
    let capture: CameraCapture | null = null;
    try {
      const result = await this.media.acquireDevice({
        deviceHandle: selected.device.deviceHandle,
        operationId: operationId(),
        kind: g.YAS_MEDIA_KIND_CAMERA,
        leaseDurationNs: MEDIA_CAPTURE_LEASE_NS,
        formats: [selected.format],
      });
      if (generation !== this.cameraGeneration) {
        await Promise.allSettled([
          this.media.releaseDevice(result.leaseHandle, operationId()),
          this.media.closeStream(result.streamHandle, operationId()),
        ]);
        track.stop();
        throw new Error("camera capture was cancelled");
      }
      if (!mediaFormatsEqual(result.selectedFormat, selected.format)) {
        await Promise.allSettled([
          this.media.releaseDevice(result.leaseHandle, operationId()),
          this.media.closeStream(result.streamHandle, operationId()),
        ]);
        throw new Error("Media camera selected an unoffered format");
      }
      this.camera = {
        kind: "camera",
        leaseHandle: result.leaseHandle,
        streamHandle: result.streamHandle,
        format: result.selectedFormat,
        sequence: 0n,
        creditFrames: 0,
        discontinuity: false,
        keyframeRequired: selected.wire !== 0,
      };
      const canEncode = () =>
        generation === this.cameraGeneration &&
        this.cameraCapture === capture &&
        (this.camera?.creditFrames ?? 0) > 0;
      const ended = () => {
        if (
          generation === this.cameraGeneration &&
          this.cameraCapture === capture
        )
          this.stopCapture("camera", "camera device ended", false);
      };
      capture =
        selected.wire === 0
          ? new MjpegCameraCapture(
              track,
              width,
              height,
              fps,
              (data, captureUs) => {
                if (
                  generation === this.cameraGeneration &&
                  this.cameraCapture === capture
                )
                  this.sendInputFrame("camera", data, captureUs, true);
              },
              ended,
              canEncode,
              cameraQuality(options.quality).jpeg,
            )
          : new WebCodecsCameraCapture(
              track,
              selected.wire,
              width,
              height,
              fps,
              (data, captureUs, keyframe) => {
                if (
                  generation === this.cameraGeneration &&
                  this.cameraCapture === capture
                )
                  this.sendInputFrame("camera", data, captureUs, keyframe);
              },
              () => {
                if (
                  generation === this.cameraGeneration &&
                  this.cameraCapture === capture &&
                  this.camera
                )
                  this.camera.discontinuity = true;
              },
              ended,
              canEncode,
              (error) => {
                if (
                  generation === this.cameraGeneration &&
                  this.cameraCapture === capture
                )
                  this.stopCapture("camera", error.message);
              },
              cameraQuality(options.quality).scale,
            );
      if (this.cameraPendingTrack === track) this.cameraPendingTrack = null;
      this.cameraCapture = capture;
      await capture.start();
      if (
        generation !== this.cameraGeneration ||
        this.cameraCapture !== capture
      ) {
        capture.stop();
        throw new Error("camera capture was cancelled");
      }
      this.options.mediaStore.publishNativeLease("camera", {
        kind: "camera",
        status: "active",
        leaseId: result.leaseHandle,
        codec: selected.wire,
        width,
        height,
        fps,
        credit: 0,
        error: null,
      });
    } catch (error) {
      if (generation === this.cameraGeneration) {
        this.stopCapture(
          "camera",
          error instanceof Error ? error.message : String(error),
        );
      } else {
        capture?.stop();
        if (this.cameraPendingTrack === track) {
          this.cameraPendingTrack = null;
          track.stop();
        }
      }
      throw error;
    }
  }

  private stopCapture(
    kind: "microphone" | "camera",
    error: string | null = null,
    stopTrack = true,
  ): void {
    if (kind === "microphone") this.microphoneGeneration++;
    else this.cameraGeneration++;
    const input = kind === "microphone" ? this.microphone : this.camera;
    if (kind === "microphone") {
      this.microphone = null;
      this.microphoneEncoder?.stop();
      this.microphoneEncoder = null;
      this.microphoneCapture?.stop(stopTrack);
      this.microphoneCapture = null;
    } else {
      this.camera = null;
      this.cameraPendingTrack?.stop();
      this.cameraPendingTrack = null;
      this.cameraCapture?.stop(stopTrack);
      this.cameraCapture = null;
      this.options.mediaStore.publishNativeCameraTrack(null);
    }
    this.options.mediaStore.publishNativeLease(kind, {
      ...emptyNativeLease(kind),
      error,
    });
    if (input && this.media) {
      void Promise.allSettled([
        this.media.releaseDevice(input.leaseHandle, operationId()),
        this.media.closeStream(input.streamHandle, operationId()),
      ]);
    }
  }

  private sendInputFrame(
    kind: "microphone" | "camera",
    payload: Uint8Array,
    captureUs: number,
    keyframe: boolean,
  ): void {
    const input = kind === "microphone" ? this.microphone : this.camera;
    if (!input || !this.media || payload.length === 0) return;
    if (input.creditFrames === 0 || (input.keyframeRequired && !keyframe)) {
      input.discontinuity = true;
      if (input.keyframeRequired) this.cameraCapture?.requestKeyframe();
      return;
    }
    const fragmentCount = Math.ceil(
      payload.length / MEDIA_FRAME_FRAGMENT_BYTES,
    );
    if (
      fragmentCount === 0 ||
      fragmentCount > MEDIA_FRAME_FRAGMENT_MAX ||
      payload.length > 4 * 1024 * 1024
    ) {
      input.discontinuity = true;
      if (kind === "camera") this.cameraCapture?.requestKeyframe();
      return;
    }
    input.sequence++;
    const flags =
      (keyframe ? g.YAS_MEDIA_FRAME_KEYFRAME : 0) |
      (!keyframe ? g.YAS_MEDIA_FRAME_DISCARDABLE : 0) |
      (input.discontinuity ? g.YAS_MEDIA_FRAME_DISCONTINUITY : 0);
    const captureTime =
      kind === "microphone"
        ? (BigInt(Math.max(0, Math.floor(captureUs))) *
            BigInt(input.format.sampleRate)) /
          1_000_000n
        : BigInt(Math.max(0, Math.floor(captureUs))) * 1_000n;
    for (
      let fragmentIndex = 0;
      fragmentIndex < fragmentCount;
      fragmentIndex++
    ) {
      this.media.sendFrame({
        streamHandle: input.streamHandle,
        sequence: input.sequence,
        captureTime,
        presentationTime: 0n,
        codecVersion: input.format.codec,
        flags,
        fragmentIndex,
        fragmentCount,
        completeLength: payload.length,
        payload: new Uint8Array(
          payload.subarray(
            fragmentIndex * MEDIA_FRAME_FRAGMENT_BYTES,
            Math.min(
              payload.length,
              (fragmentIndex + 1) * MEDIA_FRAME_FRAGMENT_BYTES,
            ),
          ),
        ),
      });
    }
    input.creditFrames--;
    input.discontinuity = false;
    if (keyframe) input.keyframeRequired = false;
    const lease =
      kind === "microphone"
        ? this.options.mediaStore.microphone
        : this.options.mediaStore.camera;
    this.options.mediaStore.publishNativeLease(kind, {
      ...lease,
      credit: input.creditFrames,
    });
  }

  private queueMedia(snapshot: YasMediaSnapshot): void {
    this.pendingMedia = snapshot;
    if (this.mediaDrain) return;
    const generation = this.mediaGeneration;
    this.mediaDrain = (async () => {
      while (!this.disposed && generation === this.mediaGeneration) {
        const next = this.pendingMedia;
        if (!next) return;
        this.pendingMedia = null;
        await this.publishMedia(next);
      }
    })()
      .catch((error: unknown) => this.report(error))
      .finally(() => {
        this.mediaDrain = null;
        if (this.pendingMedia && !this.disposed)
          this.queueMedia(this.pendingMedia);
      });
  }

  private async publishMedia(snapshot: YasMediaSnapshot): Promise<void> {
    const generation = this.mediaGeneration;
    const previous = this.mediaSnapshot;
    this.mediaSnapshot = snapshot;
    const activeMicrophone = snapshot.leases.find(
      (lease) =>
        lease.lifecycle === g.YAS_MEDIA_LEASE_ACTIVE &&
        snapshot.devices.some(
          (device) =>
            device.deviceHandle === lease.deviceHandle &&
            device.deviceKind === g.YAS_MEDIA_KIND_MICROPHONE,
        ),
    );
    const activeCamera = snapshot.leases.find(
      (lease) =>
        lease.lifecycle === g.YAS_MEDIA_LEASE_ACTIVE &&
        snapshot.devices.some(
          (device) =>
            device.deviceHandle === lease.deviceHandle &&
            device.deviceKind === g.YAS_MEDIA_KIND_CAMERA,
        ),
    );
    const screencasts = snapshot.portals.flatMap((portal) => {
      if (
        portal.portalKind !== g.YAS_MEDIA_PORTAL_KIND_SCREENCAST ||
        portal.state !== g.YAS_MEDIA_PORTAL_GRANTED ||
        portal.metadata.kind !== "grant" ||
        portal.metadata.grant.kind !== "screencastGranted"
      )
        return [];
      const appId = this.portalRequests.get(portal.portalHandle)?.metadata;
      return [
        {
          sessionId: portal.portalHandle,
          appId:
            appId?.kind === "screencast" || appId?.kind === "access"
              ? appId.appId
              : "",
          surfaceIds: portal.metadata.grant.streams.map(
            (stream) => stream.surfaceHandle,
          ),
        },
      ];
    });
    let runtimeFlags = this.supportsDesktopMedia ? RUNTIME_PORTAL_FRONTEND : 0;
    if (
      snapshot.devices.some(
        (device) => device.deviceKind === g.YAS_MEDIA_KIND_MICROPHONE,
      )
    )
      runtimeFlags |= RUNTIME_MICROPHONE;
    if (
      snapshot.devices.some(
        (device) => device.deviceKind === g.YAS_MEDIA_KIND_CAMERA,
      )
    )
      runtimeFlags |= RUNTIME_CAMERA;
    if (
      snapshot.portals.some(
        (portal) => portal.portalKind === g.YAS_MEDIA_PORTAL_KIND_ACCESS,
      )
    )
      runtimeFlags |= RUNTIME_PORTAL_ACCESS;
    if (
      snapshot.portals.some(
        (portal) => portal.portalKind === g.YAS_MEDIA_PORTAL_KIND_SCREENCAST,
      )
    )
      runtimeFlags |= RUNTIME_PORTAL_SCREENCAST;
    if (snapshot.players.length) runtimeFlags |= RUNTIME_MPRIS;
    const state: DesktopMediaState = {
      runtimeFlags,
      activeFlags:
        (activeMicrophone ? ACTIVE_MICROPHONE : 0) |
        (activeCamera ? ACTIVE_CAMERA : 0) |
        (screencasts.length ? ACTIVE_SCREENCAST : 0),
      microphoneOwner: activeMicrophone
        ? bytesHex(activeMicrophone.ownerSession)
        : 0n,
      cameraOwner: activeCamera ? bytesHex(activeCamera.ownerSession) : 0n,
      screencasts,
    };
    const liveArtwork = new Set<string>();
    const retainedArtwork = { bytes: 0 };
    const players: MprisPlayer[] = [];
    for (const player of snapshot.players) {
      players.push(
        await this.productPlayer(player, liveArtwork, retainedArtwork),
      );
    }
    if (this.disposed || generation !== this.mediaGeneration) return;
    this.options.mediaStore.replaceNativeState(state);
    this.options.mprisStore.replaceNative(players);
    this.prunePlayerArtwork(liveArtwork);
    const presentPortals = new Set(
      snapshot.portals.map((item) => item.portalHandle),
    );
    for (const handle of this.portalRequests.keys()) {
      if (!presentPortals.has(handle)) {
        this.portalRequests.delete(handle);
        this.options.mediaStore.removeNativeRequest(handle);
      }
    }
    if (!previous && this.desiredAudioBitrate !== null)
      this.sendAudioSubscribe(this.desiredAudioBitrate);
    this.options.onChanged?.();
  }

  private async productPlayer(
    record: YasMediaPlayerRecord,
    liveArtwork: Set<string>,
    retainedArtwork: { bytes: number },
  ): Promise<MprisPlayer> {
    const artworkHash = record.extensions.find(
      (extension) =>
        extension.tag === g.YAS_MEDIA_PLAYER_ALBUM_ART_HASH_EXTENSION,
    )?.value;
    const artworkUrl = mediaPlayerAlbumArtUrl(record);
    const artwork = artworkUrl
      ? ({ kind: "url", url: artworkUrl } as const)
      : await this.productArtwork(artworkHash, liveArtwork, retainedArtwork);
    return {
      playerId: record.playerHandle,
      revision: record.revision,
      trackRevision: record.revision,
      active: mediaPlayerActive(record) ?? false,
      playbackStatus:
        record.state === g.YAS_MEDIA_PLAYER_PLAYING
          ? "playing"
          : record.state === g.YAS_MEDIA_PLAYER_PAUSED
            ? "paused"
            : "stopped",
      loopStatus: "none",
      shuffle: false,
      capabilityFlags: record.flags,
      rate: 1,
      minimumRate: 1,
      maximumRate: 1,
      volume: 1,
      positionUs: safeSignedNumber(record.positionUs),
      lengthUs: safeSignedNumber(record.durationUs),
      identity: record.identity,
      desktopEntry: record.desktopEntry,
      title: record.title,
      album: record.album,
      artists: record.artist ? [record.artist] : [],
      artwork,
      receivedAtMs: monotonicNow(),
    };
  }

  private async productArtwork(
    hash: Uint8Array | undefined,
    live: Set<string>,
    retained: { bytes: number },
  ): Promise<MprisPlayer["artwork"]> {
    if (!hash || hash.length !== 32 || !this.media) return null;
    const key = bytesHex(hash);
    const cached = this.playerArtwork.get(key);
    if (cached) {
      if (
        !live.has(key) &&
        (live.size >= MAX_PLAYER_ARTWORK_ITEMS ||
          retained.bytes + cached.bytes > MAX_PLAYER_ARTWORK_BYTES)
      )
        return null;
      if (!live.has(key)) retained.bytes += cached.bytes;
      live.add(key);
      this.playerArtwork.delete(key);
      this.playerArtwork.set(key, cached);
      return cached.artwork;
    }
    if (
      live.size >= MAX_PLAYER_ARTWORK_ITEMS ||
      retained.bytes >= MAX_PLAYER_ARTWORK_BYTES
    )
      return null;
    try {
      const content = await this.media.fetchAsset(hash);
      const remaining = MAX_PLAYER_ARTWORK_BYTES - retained.bytes;
      if (
        content.byteLength === 0n ||
        content.byteLength > BigInt(MAX_PLAYER_ARTWORK_SINGLE_BYTES) ||
        content.byteLength > BigInt(remaining)
      )
        return null;
      const bytes = await content.bytes();
      if (
        bytes.length === 0 ||
        bytes.length > MAX_PLAYER_ARTWORK_SINGLE_BYTES ||
        bytes.length > remaining
      )
        return null;
      if (!safePngDimensions(bytes)) return null;
      const entry = {
        artwork: {
          kind: "png" as const,
          png: new Uint8Array(bytes),
        },
        bytes: bytes.length,
      };
      this.evictPlayerArtwork(live, entry.bytes);
      this.playerArtwork.set(key, entry);
      this.playerArtworkBytes += entry.bytes;
      retained.bytes += entry.bytes;
      live.add(key);
      return entry.artwork;
    } catch {
      return null;
    }
  }

  private evictPlayerArtwork(
    live: ReadonlySet<string>,
    incoming: number,
  ): void {
    while (
      this.playerArtwork.size >= MAX_PLAYER_ARTWORK_ITEMS ||
      this.playerArtworkBytes + incoming > MAX_PLAYER_ARTWORK_BYTES
    ) {
      const candidate = [...this.playerArtwork.entries()].find(
        ([key]) => !live.has(key),
      );
      if (!candidate) return;
      this.playerArtwork.delete(candidate[0]);
      this.playerArtworkBytes -= candidate[1].bytes;
    }
  }

  private prunePlayerArtwork(live: ReadonlySet<string>): void {
    for (const [key, entry] of this.playerArtwork) {
      if (live.has(key)) continue;
      this.playerArtwork.delete(key);
      this.playerArtworkBytes -= entry.bytes;
    }
  }

  private async publishPortalRequest(
    request: YasMediaPortalRequest,
  ): Promise<void> {
    if (!this.media) return;
    this.portalRequests.set(request.portalHandle, request);
    const serverNow =
      this.options.session.estimatedServerMonotonicNs() ??
      this.options.session.hello?.serverMonotonicNs ??
      0n;
    const deadline = request.metadata.deadlineServerNs;
    const deadlineMs = Number(
      ((deadline > serverNow ? deadline - serverNow : 1n) + 999_999n) /
        1_000_000n,
    );
    let product: PortalRequest;
    if (request.metadata.kind === "access") {
      product = {
        kind: "access",
        requestId: request.portalHandle,
        deadlineMs: Math.min(deadlineMs, 0xffff_ffff),
        parentSurfaceId: request.metadata.parentSurfaceHandle,
        appId: request.metadata.appId,
        title: request.metadata.title,
        subtitle: request.metadata.subtitle,
        body: request.metadata.body,
        denyLabel: request.metadata.denyLabel,
        grantLabel: request.metadata.grantLabel,
        iconName: request.metadata.iconName,
        choices: request.metadata.choices.map((choice) => ({
          id: choice.id,
          label: choice.label,
          initialValue: choice.initial,
          options: choice.options,
        })),
      };
    } else {
      const candidates = [];
      for (const candidate of request.metadata.candidates) {
        let thumbnailPng = new Uint8Array();
        if (candidate.thumbnailHash) {
          try {
            const bytes = await (
              await this.media!.fetchAsset(candidate.thumbnailHash)
            ).bytes();
            if (bytes.length <= 64 * 1024) thumbnailPng = new Uint8Array(bytes);
          } catch {
            // An optional preview asset cannot suppress the permission prompt.
          }
        }
        candidates.push({
          surfaceId: candidate.surfaceHandle,
          width: candidate.width,
          height: candidate.height,
          title: candidate.title,
          appId: candidate.appId,
          thumbnailPng,
        });
      }
      if (this.portalRequests.get(request.portalHandle) !== request) return;
      product = {
        kind: "screencast",
        requestId: request.portalHandle,
        deadlineMs: Math.min(deadlineMs, 0xffff_ffff),
        parentSurfaceId: request.metadata.parentSurfaceHandle,
        appId: request.metadata.appId,
        multiple: request.metadata.multiple,
        candidates,
      };
    }
    this.options.mediaStore.publishNativeRequest(product);
  }

  private handleMediaFrameAck(ack: YasMediaFrameAck): void {
    const input =
      this.microphone?.streamHandle === ack.streamHandle
        ? this.microphone
        : this.camera?.streamHandle === ack.streamHandle
          ? this.camera
          : null;
    if (!input) return;
    input.creditFrames = Math.min(64, ack.desiredCreditFrames);
    const current =
      input.kind === "microphone"
        ? this.options.mediaStore.microphone
        : this.options.mediaStore.camera;
    this.options.mediaStore.publishNativeLease(input.kind, {
      ...current,
      credit: input.creditFrames,
    });
  }

  private handleMediaStreamStatus(status: YasMediaStreamStatus): void {
    if (status.streamHandle === this.camera?.streamHandle) {
      if (status.flags & g.YAS_MEDIA_STREAM_KEYFRAME_REQUIRED) {
        this.camera.keyframeRequired = true;
        this.cameraCapture?.requestKeyframe();
      }
      if (
        status.status === g.YAS_MEDIA_STREAM_CLOSED ||
        status.status === g.YAS_MEDIA_STREAM_ERROR
      )
        this.stopCapture("camera", "camera stream closed");
    }
    if (
      status.streamHandle === this.microphone?.streamHandle &&
      (status.status === g.YAS_MEDIA_STREAM_CLOSED ||
        status.status === g.YAS_MEDIA_STREAM_ERROR)
    )
      this.stopCapture("microphone", "microphone stream closed");
    if (
      status.streamHandle === this.audioOutput?.streamHandle &&
      (status.status === g.YAS_MEDIA_STREAM_CLOSED ||
        status.status === g.YAS_MEDIA_STREAM_ERROR)
    )
      this.sendAudioUnsubscribe();
  }

  private handleMediaFrame(frame: YasMediaFrame): void {
    const output = this.audioOutput;
    if (!output || frame.streamHandle !== output.streamHandle) return;
    if (frame.codecVersion !== g.YAS_MEDIA_CODEC_OPUS)
      throw new YasProtocolError("Media audio output changed codec");
    if (frame.fragmentIndex === 0) {
      if (
        output.reassembly ||
        frame.sequence <= output.consumedSequence ||
        frame.fragmentCount > MEDIA_AUDIO_FRAGMENT_MAX ||
        frame.completeLength > MEDIA_AUDIO_RETAINED_MAX
      )
        throw new YasProtocolError("invalid Media audio frame start");
      const lease = this.options.session.receiveBudget.reserveExact(
        BigInt(frame.completeLength),
      );
      output.reassembly = {
        sequence: frame.sequence,
        captureTime: frame.captureTime,
        presentationTime: frame.presentationTime,
        codecVersion: frame.codecVersion,
        flags: frame.flags,
        fragmentCount: frame.fragmentCount,
        nextFragment: 0,
        completeLength: frame.completeLength,
        bytes: new Uint8Array(frame.completeLength),
        received: 0,
        lease,
      };
    }
    const pending = output.reassembly;
    if (
      !pending ||
      frame.sequence !== pending.sequence ||
      frame.captureTime !== pending.captureTime ||
      frame.presentationTime !== pending.presentationTime ||
      frame.codecVersion !== pending.codecVersion ||
      frame.flags !== pending.flags ||
      frame.fragmentCount !== pending.fragmentCount ||
      frame.completeLength !== pending.completeLength ||
      frame.fragmentIndex !== pending.nextFragment ||
      pending.received + frame.payload.length > pending.completeLength
    ) {
      pending?.lease.release();
      output.reassembly = null;
      throw new YasProtocolError("invalid fragmented Media audio frame");
    }
    pending.bytes.set(frame.payload, pending.received);
    pending.received += frame.payload.length;
    pending.nextFragment++;
    if (pending.nextFragment !== pending.fragmentCount) return;
    output.reassembly = null;
    try {
      if (pending.received !== pending.completeLength)
        throw new YasProtocolError("incomplete Media audio frame");
      const packets = decodeOpusPackets(pending.bytes);
      const samplePosition = pending.presentationTime || pending.captureTime;
      const sampleRate = BigInt(output.sampleRate);
      let timestampMs =
        Number(samplePosition / sampleRate) * 1_000 +
        (Number(samplePosition % sampleRate) * 1_000) / output.sampleRate;
      for (const packet of packets) {
        this.options.audioPlayer.handleAudioFrame(
          timestampMs,
          pending.flags,
          packet,
        );
        timestampMs +=
          (nativeOpusPacketSamples(packet, output.sampleRate) * 1_000) /
          output.sampleRate;
      }
      output.consumedSequence = pending.sequence;
      this.media!.sendFrameAck({
        streamHandle: output.streamHandle,
        consumedSequence: output.consumedSequence,
        queueDepth: 0,
        desiredCreditFrames: MEDIA_OUTPUT_CREDIT,
      });
    } finally {
      pending.lease.release();
    }
  }

  private queueAudio(operation: () => void | Promise<void>): void {
    const generation = this.audioOutputGeneration;
    // Audio controls describe desired state. Keep at most the in-flight
    // transition and the latest replacement instead of retaining one closure
    // for every UI toggle while an OPEN_OUTPUT/CLOSE_STREAM request is slow.
    this.pendingAudioOperation = async () => {
      if (generation !== this.audioOutputGeneration) return;
      await operation();
    };
    if (this.audioOutputDrain) return;
    this.audioOutputDrain = (async () => {
      while (true) {
        const next = this.pendingAudioOperation;
        if (!next) return;
        this.pendingAudioOperation = null;
        await next();
      }
    })()
      .catch((error: unknown) => this.report(error))
      .finally(() => {
        this.audioOutputDrain = null;
        if (this.pendingAudioOperation)
          this.queueAudio(this.pendingAudioOperation);
      });
  }

  private async closeAudioOutput(): Promise<void> {
    const output = this.audioOutput;
    this.audioOutput = null;
    if (output?.reassembly) output.reassembly.lease.release();
    this.options.audioPlayer.reset();
    if (output && this.media)
      await this.media.closeStream(output.streamHandle, operationId());
  }

  private desktopController(): NativeDesktopController {
    return {
      subscribe: () => undefined,
      activate: (trayId) => {
        void this.reported(() =>
          this.trayAction(trayId, g.YAS_DESKTOP_TRAY_ACTION_ACTIVATE, 0, 0n, 0),
        );
      },
      secondaryActivate: (trayId) => {
        void this.reported(() =>
          this.trayAction(
            trayId,
            g.YAS_DESKTOP_TRAY_ACTION_SECONDARY_ACTIVATE,
            0,
            0n,
            0,
          ),
        );
      },
      openMenu: (trayId, menuRevision) => {
        void this.reported(() => this.openMenu(trayId, menuRevision));
      },
      scroll: (trayId, delta, horizontal) => {
        void this.reported(() =>
          this.trayAction(
            trayId,
            g.YAS_DESKTOP_TRAY_ACTION_SCROLL,
            horizontal ? g.YAS_DESKTOP_TRAY_ACTION_SCROLL_HORIZONTAL : 0,
            0n,
            delta,
          ),
        );
      },
      clickMenuItem: (trayId, menuRevision, itemId) => {
        void this.reported(() =>
          this.trayAction(
            trayId,
            g.YAS_DESKTOP_TRAY_ACTION_MENU_ITEM,
            0,
            requireNativeId(itemId, "Desktop menu item"),
            0,
            menuRevision,
          ),
        );
      },
      invokeDefault: (notificationId, revision) => {
        void this.reported(() =>
          this.notificationAction(
            notificationId,
            revision,
            g.YAS_DESKTOP_NOTIFICATION_ACTION_DEFAULT,
            0n,
            "",
          ),
        );
      },
      invokeAction: (notificationId, revision, key) => {
        void this.reported(() =>
          this.notificationAction(
            notificationId,
            revision,
            g.YAS_DESKTOP_NOTIFICATION_ACTION_ACTION,
            parseActionKey(key),
            "",
          ),
        );
      },
      dismiss: (notificationId, revision) => {
        void this.reported(() =>
          this.notificationAction(
            notificationId,
            revision,
            g.YAS_DESKTOP_NOTIFICATION_ACTION_DISMISS,
            0n,
            "",
          ),
        );
      },
    };
  }

  private async trayAction(
    trayId: DesktopId,
    actionKind: number,
    flags: number,
    itemHandle: bigint,
    value: number,
    revision?: DesktopRevision,
  ): Promise<void> {
    if (!this.desktop) throw new Error("Desktop family unavailable");
    const handle = requireNativeId(trayId, "Desktop tray");
    const record = this.desktop.catalog.snapshot.trays.find(
      (candidate) => candidate.trayHandle === handle,
    );
    if (!record) throw new Error("Desktop tray no longer exists");
    await this.desktop.trayAction({
      trayHandle: handle,
      trayRevision: record.revision,
      menuRevision:
        revision === undefined
          ? record.menuRevision
          : requireNativeRevision(revision, "Desktop menu"),
      operationId: operationId(),
      actionKind,
      flags,
      value,
      itemHandle,
    });
  }

  private async openMenu(
    trayId: DesktopId,
    menuRevision: DesktopRevision,
  ): Promise<void> {
    if (!this.desktop) throw new Error("Desktop family unavailable");
    const handle = requireNativeId(trayId, "Desktop tray");
    const record = this.desktop.catalog.snapshot.trays.find(
      (candidate) => candidate.trayHandle === handle,
    );
    if (!record) throw new Error("Desktop tray no longer exists");
    const revision =
      menuRevision === 0n
        ? record.menuRevision
        : requireNativeRevision(menuRevision, "Desktop menu");
    const content = await this.desktop.getMenu(
      handle,
      record.revision,
      revision,
    );
    const menu = await content.menu();
    this.options.desktopStore.publishNativeMenu({
      trayId: menu.trayHandle,
      trayRevision: menu.trayRevision,
      menuRevision: menu.menuRevision,
      status: 0,
      nodes: menu.nodes
        .filter((node) => node.kind !== g.YAS_DESKTOP_MENU_NODE_ROOT)
        .map((node) => ({
          id: node.actionHandle || node.nodeHandle,
          parentId: node.parentHandle,
          position: Math.min(node.position, 0xffff),
          flags:
            (node.flags & g.YAS_DESKTOP_MENU_VISIBLE ? MENU_NODE_VISIBLE : 0) |
            (node.flags & g.YAS_DESKTOP_MENU_ENABLED ? MENU_NODE_ENABLED : 0) |
            (node.kind === g.YAS_DESKTOP_MENU_NODE_SEPARATOR
              ? MENU_NODE_SEPARATOR
              : 0) |
            (node.kind === g.YAS_DESKTOP_MENU_NODE_SUBMENU
              ? MENU_NODE_SUBMENU
              : 0) |
            (node.flags & g.YAS_DESKTOP_MENU_CHECKED
              ? MENU_NODE_CHECKMARK
              : 0) |
            (node.flags & g.YAS_DESKTOP_MENU_RADIO ? MENU_NODE_RADIO : 0),
          toggleState: node.flags & g.YAS_DESKTOP_MENU_CHECKED ? 1 : 0,
          label: node.label,
          icon: EMPTY_IMAGE,
        })),
    });
  }

  private async notificationAction(
    notificationId: DesktopId,
    revision: DesktopRevision,
    actionKind: number,
    actionHandle: bigint,
    reply: string,
  ): Promise<void> {
    if (!this.desktop) throw new Error("Desktop family unavailable");
    await this.desktop.notificationAction({
      notificationHandle: requireNativeId(
        notificationId,
        "Desktop notification",
      ),
      revision: requireNativeRevision(revision, "Desktop notification"),
      actionKind,
      actionHandle,
      operationId: operationId(),
      reply,
    });
  }

  private queueDesktop(snapshot: YasDesktopSnapshot): void {
    this.pendingDesktop = snapshot;
    if (this.desktopDrain) return;
    const generation = this.desktopGeneration;
    this.desktopDrain = (async () => {
      while (!this.disposed && generation === this.desktopGeneration) {
        const next = this.pendingDesktop;
        if (!next) return;
        this.pendingDesktop = null;
        await this.publishDesktop(next);
      }
    })()
      .catch((error: unknown) => this.report(error))
      .finally(() => {
        this.desktopDrain = null;
        if (this.pendingDesktop && !this.disposed)
          this.queueDesktop(this.pendingDesktop);
      });
  }

  private async publishDesktop(snapshot: YasDesktopSnapshot): Promise<void> {
    const generation = this.desktopGeneration;
    const liveAssets = new Set<string>();
    const retainedAssets = { bytes: 0 };
    const trays: TrayItem[] = [];
    for (const record of snapshot.trays) {
      const icon = await this.asset(
        record.iconHash,
        liveAssets,
        retainedAssets,
      );
      trays.push(desktopTray(record, icon));
    }
    const notifications: DesktopNotification[] = [];
    for (const record of snapshot.notifications) {
      const icon = await this.asset(
        record.applicationIconHash,
        liveAssets,
        retainedAssets,
      );
      const image = await this.asset(
        record.contentImageHash,
        liveAssets,
        retainedAssets,
      );
      notifications.push(
        desktopNotification(this.options.session, record, icon, image),
      );
    }
    if (this.disposed || generation !== this.desktopGeneration) return;
    this.pruneAssets(liveAssets);
    this.options.desktopStore.replaceNative(
      trays,
      notifications,
      this.desktopInitial,
    );
    this.desktopInitial = false;
    this.options.onChanged?.();
  }

  private async asset(
    hash: Uint8Array | null,
    live: Set<string>,
    retained: { bytes: number },
  ): Promise<DesktopImage> {
    if (!hash || hash.every((byte) => byte === 0) || !this.desktop)
      return EMPTY_IMAGE;
    const key = bytesHex(hash);
    const cached = this.assets.get(key);
    if (cached) {
      if (
        !live.has(key) &&
        (live.size >= MAX_ASSET_ITEMS ||
          retained.bytes + cached.bytes > MAX_ASSET_BYTES)
      )
        return EMPTY_IMAGE;
      if (!live.has(key)) retained.bytes += cached.bytes;
      live.add(key);
      this.assets.delete(key);
      this.assets.set(key, cached);
      return cached.image;
    }
    if (live.size >= MAX_ASSET_ITEMS || retained.bytes >= MAX_ASSET_BYTES)
      return EMPTY_IMAGE;
    try {
      const content = await this.desktop.fetchAsset(new Uint8Array(hash));
      const remaining = MAX_ASSET_BYTES - retained.bytes;
      if (
        content.byteLength === 0n ||
        content.byteLength > BigInt(MAX_ASSET_SINGLE_BYTES) ||
        content.byteLength > BigInt(remaining)
      )
        return EMPTY_IMAGE;
      const bytes = await content.bytes();
      if (
        bytes.length === 0 ||
        bytes.length > MAX_ASSET_SINGLE_BYTES ||
        bytes.length > remaining
      )
        return EMPTY_IMAGE;
      const dimensions = safePngDimensions(bytes);
      if (!dimensions) return EMPTY_IMAGE;
      const image = {
        width: dimensions.width,
        height: dimensions.height,
        png: new Uint8Array(bytes),
      };
      this.evictAssets(live, image.png.length);
      this.assets.set(key, { image, bytes: image.png.length });
      this.assetBytes += image.png.length;
      retained.bytes += image.png.length;
      live.add(key);
      return image;
    } catch {
      return EMPTY_IMAGE;
    }
  }

  private evictAssets(live: ReadonlySet<string>, incoming: number): void {
    while (
      this.assets.size >= MAX_ASSET_ITEMS ||
      this.assetBytes + incoming > MAX_ASSET_BYTES
    ) {
      const candidate = [...this.assets.entries()].find(
        ([key]) => !live.has(key),
      );
      if (!candidate) return;
      this.assets.delete(candidate[0]);
      this.assetBytes -= candidate[1].bytes;
    }
  }

  private pruneAssets(live: ReadonlySet<string>): void {
    for (const [key, entry] of this.assets) {
      if (live.has(key)) continue;
      this.assets.delete(key);
      this.assetBytes -= entry.bytes;
    }
  }

  private publishClients(snapshot: YasClientSnapshot): void {
    const hello = this.options.session.hello;
    if (!hello) return;
    const serverNow =
      this.options.session.estimatedServerMonotonicNs() ??
      hello.serverMonotonicNs;
    const catalog: YasClientList = {
      selfId: bytesHex(hello.sessionId),
      clients: snapshot.clients.map((record) =>
        productClient(serverNow, record),
      ),
    };
    this.clientSnapshot = catalog;
    for (const subscriber of this.clientListeners) subscriber.listener(catalog);
    this.options.onChanged?.();
  }

  private async reported<T>(operation: () => Promise<T>): Promise<T> {
    try {
      return await operation();
    } catch (error) {
      this.report(error);
      throw error;
    }
  }

  private report(error: unknown): void {
    const value = error instanceof Error ? error : new Error(String(error));
    this.options.onError?.(value);
  }
}

function desktopTray(
  record: YasDesktopTrayRecord,
  icon: DesktopImage,
): TrayItem {
  return {
    trayId: record.trayHandle,
    revision: record.revision,
    status: record.status,
    category: 0,
    flags: record.menuRevision === 0n ? 0 : TRAY_HAS_MENU,
    appId: "",
    title: record.title,
    tooltipTitle: record.title,
    tooltipBody: "",
    icon,
  };
}

function desktopNotification(
  session: YasConnection,
  record: YasDesktopNotificationRecord,
  icon: DesktopImage,
  image: DesktopImage,
): DesktopNotification {
  const remaining = session.nanosecondsUntilServerTime(record.expiresServerNs);
  const timeoutMs = Number(
    remaining / 1_000_000n > 0xffff_ffffn
      ? 0xffff_ffffn
      : remaining / 1_000_000n,
  );
  return {
    notificationId: record.notificationHandle,
    revision: record.revision,
    urgency: record.urgency,
    flags:
      (record.flags & g.YAS_DESKTOP_NOTIFICATION_RESIDENT
        ? NOTIFICATION_RESIDENT
        : 0) |
      (record.flags & g.YAS_DESKTOP_NOTIFICATION_TRANSIENT
        ? NOTIFICATION_TRANSIENT
        : 0),
    timeoutMs,
    appName: record.application,
    desktopEntry: "",
    summary: record.summary,
    body: record.body,
    icon,
    image,
    actions: [
      { key: "default", label: "" },
      ...record.actions.map((action) => ({
        key: `native:${action.actionHandle}`,
        label: action.label,
      })),
    ],
  };
}

function productClient(
  serverNow: bigint,
  record: YasClientRecord,
): YasClientInfo {
  const active = record.activeSubscriptions;
  const details = new Map(
    (record.auxiliarySubscriptionDetails?.entries ?? []).map((entry) => [
      `${entry.family}:${entry.subscriptionId}`,
      entry,
    ]),
  );
  const subscriptions: YasClientAuxSubscription[] = (
    active?.auxiliary ?? []
  ).map((entry) => {
    const detail = details.get(`${entry.family}:${entry.subscriptionId}`);
    return {
      kind: entry.family,
      id: entry.resourceHandle,
      subscriptionId: entry.subscriptionId,
      resource: detail?.resource,
      requestFlags: detail?.requestFlags,
      stateWatchFlags: detail?.stateWatchFlags,
    };
  });
  return {
    id: bytesHex(record.sessionId),
    ageSeconds: Number(
      (serverNow > record.connectedServerNs
        ? serverNow - record.connectedServerNs
        : 0n) / 1_000_000_000n,
    ),
    outboundBytesPerSecond: safeNumber(
      record.bandwidthRates?.sentBytesPerSecond ?? 0n,
    ),
    inboundBytesPerSecond: safeNumber(
      record.bandwidthRates?.receivedBytesPerSecond ?? 0n,
    ),
    subscriptions,
    terminals: (active?.terminals ?? []).map((entry) => ({
      ptyId: entry.terminalHandle,
      rows: entry.rows || null,
      cols: entry.columns || null,
    })),
    surfaces: (active?.surfaces ?? []).map((entry) => ({
      surfaceId: entry.surfaceHandle,
      width: entry.width || null,
      height: entry.height || null,
      scale120: entry.scale120 || null,
    })),
    origin: productOrigin(record.origin),
  };
}

function productOrigin(origin: YasClientOrigin): ProductClientOrigin {
  switch (origin.kind) {
    case g.YAS_CLIENT_ORIGIN_UNIX:
      if (!("peerPid" in origin)) break;
      return {
        kind: "unix",
        peerPid: origin.peerPid,
        peerUid: origin.peerUid,
        peerGid: origin.peerGid,
      };
    case g.YAS_CLIENT_ORIGIN_SSH:
      if (!("remoteAddress" in origin)) break;
      return {
        kind: "ssh",
        remoteAddress: origin.remoteAddress,
        username: origin.username,
      };
    case g.YAS_CLIENT_ORIGIN_EDGE:
      if (!("subject" in origin)) break;
      return { kind: "edge", subject: origin.subject, issuer: origin.issuer };
    case g.YAS_CLIENT_ORIGIN_RELAY:
      if (!("routeHandle" in origin)) break;
      return {
        kind: "relay",
        routeHandle: origin.routeHandle,
        generation: origin.generation,
        depth: origin.depth,
      };
    case g.YAS_CLIENT_ORIGIN_WEBRTC:
      if (!("peerId" in origin)) break;
      return { kind: "webrtc", peerId: origin.peerId };
    case g.YAS_CLIENT_ORIGIN_EXTENSION:
      if (!("extensionId" in origin)) break;
      return {
        kind: "extension",
        extensionId: origin.extensionId,
        definitionRevision: origin.definitionRevision,
        attempt: origin.attempt,
        taskId: origin.taskId,
        name: origin.name,
      };
  }
  return { kind: "unknown", originKind: origin.kind };
}

type RequiredOperation = readonly [
  frameClass: number,
  kind: number,
  serverSends?: boolean,
];

function operationsAvailable(
  session: YasConnection,
  family: number,
  operations: readonly RequiredOperation[],
): boolean {
  try {
    session.family(family);
    return operations.every(([frameClass, kind, serverSends = false]) =>
      session.operationAdvertised(family, frameClass, kind, serverSends),
    );
  } catch (error) {
    if (
      error instanceof YasResultError &&
      (error.status === g.YAS_STATUS_UNSUPPORTED ||
        error.status === g.YAS_STATUS_UNAVAILABLE)
    )
      return false;
    throw error;
  }
}

function requireNativeId(value: DesktopId, context: string): bigint {
  if (typeof value !== "bigint" || value === 0n)
    throw new Error(`${context} is not a native handle`);
  return value;
}

function requireNativeRevision(
  value: DesktopRevision,
  context: string,
): bigint {
  if (typeof value !== "bigint" || value === 0n)
    throw new Error(`${context} revision is not native`);
  return value;
}

function parseActionKey(key: string): bigint {
  if (!/^native:[1-9]\d{0,19}$/.test(key))
    throw new Error("Desktop action is not a native handle");
  const value = BigInt(key.slice("native:".length));
  if (value > 0xffff_ffff_ffff_ffffn)
    throw new Error("Desktop action handle exceeds u64");
  return value;
}

let nextOperation = 1n;
function operationId(): Uint8Array {
  const value = new Uint8Array(16);
  globalThis.crypto?.getRandomValues(value);
  if (value.some((byte) => byte !== 0)) return value;
  new DataView(value.buffer).setBigUint64(8, nextOperation++, true);
  return value;
}

function bytesHex(value: Uint8Array): string {
  return Array.from(value, (byte) => byte.toString(16).padStart(2, "0")).join(
    "",
  );
}

function decodeSessionId(value: string): Uint8Array {
  if (!/^[0-9a-f]{32}$/.test(value))
    throw new Error(
      "Client session ID must be 32 lowercase hexadecimal digits",
    );
  const output = new Uint8Array(16);
  for (let index = 0; index < output.length; index++)
    output[index] = Number.parseInt(value.slice(index * 2, index * 2 + 2), 16);
  if (output.every((byte) => byte === 0))
    throw new Error("Client session ID is zero");
  return output;
}

function safeNumber(value: bigint): number {
  return Number(
    value > BigInt(Number.MAX_SAFE_INTEGER) ? Number.MAX_SAFE_INTEGER : value,
  );
}

function safeSignedNumber(value: bigint): number {
  const maximum = BigInt(Number.MAX_SAFE_INTEGER);
  const minimum = BigInt(Number.MIN_SAFE_INTEGER);
  return Number(value > maximum ? maximum : value < minimum ? minimum : value);
}

function monotonicNow(): number {
  return globalThis.performance?.now?.() ?? Date.now();
}

function emptyNativeLease(kind: "microphone" | "camera"): MediaLeaseState {
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

function findDeviceFormat(
  devices: readonly YasMediaDeviceRecord[],
  kind: number,
  codecs: readonly number[],
  acceptable: (format: YasMediaFormat) => boolean,
): { device: YasMediaDeviceRecord; format: YasMediaFormat } | undefined {
  for (const codec of codecs) {
    for (const device of devices) {
      if (
        device.deviceKind !== kind ||
        device.state === g.YAS_MEDIA_DEVICE_UNAVAILABLE
      )
        continue;
      const format = device.formats.find(
        (candidate) => candidate.codec === codec && acceptable(candidate),
      );
      if (format) return { device, format };
    }
  }
  return undefined;
}

function mediaFormatsEqual(a: YasMediaFormat, b: YasMediaFormat): boolean {
  return (
    a.codec === b.codec &&
    a.channels === b.channels &&
    a.sampleRate === b.sampleRate &&
    a.width === b.width &&
    a.height === b.height &&
    a.frameRateMilli === b.frameRateMilli
  );
}

function nativeCameraCodec(codec: CameraWireCodec): number {
  return [
    g.YAS_MEDIA_CODEC_MJPEG,
    g.YAS_MEDIA_CODEC_H264,
    g.YAS_MEDIA_CODEC_AV1,
    g.YAS_MEDIA_CODEC_H264_444,
    g.YAS_MEDIA_CODEC_AV1_444,
  ][codec]!;
}

/** Materialize the viewer's actual camera mode from a catalogue codec entry. */
export function cameraCaptureFormat(
  advertised: YasMediaFormat,
  width: number,
  height: number,
  fps: number,
): YasMediaFormat {
  return {
    ...advertised,
    width,
    height,
    frameRateMilli: fps * 1_000,
  };
}

function evenDimension(value: number, maximum: number): number {
  const selected = Math.max(2, Math.min(maximum, Math.round(value)));
  return selected - (selected % 2);
}

function encodeOpusPackets(packets: readonly Uint8Array[]): Uint8Array {
  if (packets.length === 0 || packets.length > 0xffff)
    throw new YasProtocolError("invalid Media Opus packet count");
  const writer = new YasWriter().u16(packets.length).u16(0);
  for (const packet of packets) {
    if (packet.length === 0 || packet.length > g.YAS_MEDIA_MAX_PACKET_BYTES)
      throw new YasProtocolError("invalid Media Opus packet");
    writer.bytesU16(packet);
  }
  return writer.finish();
}

function decodeOpusPackets(payload: Uint8Array): readonly Uint8Array[] {
  const cursor = new YasCursor(payload);
  const count = cursor.u16("Media Opus packet count");
  if (
    cursor.u16("Media Opus reserved") !== 0 ||
    count === 0 ||
    count > Math.floor(cursor.remaining / 2)
  )
    throw new YasProtocolError("invalid Media Opus packet count");
  const packets: Uint8Array[] = [];
  for (let index = 0; index < count; index++) {
    const packet = cursor.bytesU16("Media Opus packet");
    if (packet.length === 0 || packet.length > g.YAS_MEDIA_MAX_PACKET_BYTES)
      throw new YasProtocolError("invalid Media Opus packet");
    packets.push(new Uint8Array(packet));
  }
  cursor.end("Media Opus payload");
  return packets;
}

/** Decode the duration carried by one complete Opus packet's TOC. */
export function nativeOpusPacketSamples(
  packet: Uint8Array,
  sampleRate = Number(g.YAS_MEDIA_WIRE_SAMPLE_RATE),
): number {
  if (packet.length === 0 || sampleRate <= 0 || !Number.isInteger(sampleRate))
    throw new YasProtocolError("invalid Media Opus packet");
  const toc = packet[0]!;
  let samplesPerFrame: number;
  if (toc & 0x80) {
    samplesPerFrame = (sampleRate << ((toc >>> 3) & 0x03)) / 400;
  } else if ((toc & 0x60) === 0x60) {
    samplesPerFrame = toc & 0x08 ? sampleRate / 50 : sampleRate / 100;
  } else {
    const configuration = (toc >>> 3) & 0x03;
    samplesPerFrame =
      configuration === 3
        ? (sampleRate * 60) / 1_000
        : (sampleRate << configuration) / 100;
  }
  const code = toc & 0x03;
  const frameCount =
    code === 0
      ? 1
      : code === 1 || code === 2
        ? 2
        : packet.length >= 2
          ? packet[1]! & 0x3f
          : 0;
  const samples = samplesPerFrame * frameCount;
  if (
    frameCount === 0 ||
    !Number.isInteger(samples) ||
    samples <= 0 ||
    samples > (sampleRate * 120) / 1_000
  )
    throw new YasProtocolError("invalid Media Opus packet duration");
  return samples;
}

function nativeMprisAction(
  action: MprisAction,
): { action: number; value: bigint } | null {
  switch (action.kind) {
    case "select":
      return null;
    case "play":
      return { action: g.YAS_MEDIA_PLAYER_ACTION_PLAY, value: 0n };
    case "pause":
      return { action: g.YAS_MEDIA_PLAYER_ACTION_PAUSE, value: 0n };
    case "playPause":
      return { action: g.YAS_MEDIA_PLAYER_ACTION_PLAY_PAUSE, value: 0n };
    case "stop":
      return { action: g.YAS_MEDIA_PLAYER_ACTION_STOP, value: 0n };
    case "next":
      return { action: g.YAS_MEDIA_PLAYER_ACTION_NEXT, value: 0n };
    case "previous":
      return { action: g.YAS_MEDIA_PLAYER_ACTION_PREVIOUS, value: 0n };
    case "seek":
      return {
        action: g.YAS_MEDIA_PLAYER_ACTION_SEEK,
        value: BigInt(Math.round(action.offsetUs)),
      };
    case "setPosition":
      return {
        action: g.YAS_MEDIA_PLAYER_ACTION_SET_POSITION,
        value: BigInt(Math.round(action.positionUs)),
      };
    case "volume":
      return {
        action: g.YAS_MEDIA_PLAYER_ACTION_SET_VOLUME,
        value: BigInt(Math.round(action.volume * 1_000_000)),
      };
    case "shuffle":
      return {
        action: g.YAS_MEDIA_PLAYER_ACTION_SET_SHUFFLE,
        value: action.shuffle ? 1n : 0n,
      };
    case "loopStatus":
      return {
        action: g.YAS_MEDIA_PLAYER_ACTION_SET_LOOP,
        value:
          action.loopStatus === "track"
            ? 1n
            : action.loopStatus === "playlist"
              ? 2n
              : 0n,
      };
    case "rate":
      return {
        action: g.YAS_MEDIA_PLAYER_ACTION_SET_RATE,
        value: BigInt(Math.round(action.rate * 1_000_000)),
      };
    case "raise":
      return { action: g.YAS_MEDIA_PLAYER_ACTION_RAISE, value: 0n };
  }
}

function pngDimensions(
  bytes: Uint8Array,
): { width: number; height: number } | null {
  if (
    bytes.length < 24 ||
    bytes[0] !== 0x89 ||
    bytes[1] !== 0x50 ||
    bytes[2] !== 0x4e ||
    bytes[3] !== 0x47 ||
    bytes[12] !== 0x49 ||
    bytes[13] !== 0x48 ||
    bytes[14] !== 0x44 ||
    bytes[15] !== 0x52
  )
    return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const width = view.getUint32(16, false);
  const height = view.getUint32(20, false);
  return width && height ? { width, height } : null;
}

function safePngDimensions(
  bytes: Uint8Array,
): { width: number; height: number } | null {
  const dimensions = pngDimensions(bytes);
  if (
    !dimensions ||
    dimensions.width > MAX_IMAGE_DIMENSION ||
    dimensions.height > MAX_IMAGE_DIMENSION ||
    dimensions.width * dimensions.height > MAX_IMAGE_PIXELS
  )
    return null;
  return dimensions;
}
