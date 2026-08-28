import { AudioPlayer } from "./AudioPlayer";
import { serverPlatform, type YasPlatform } from "./yas/core";
import { DesktopStore } from "./desktopModel";
import { EXIT_STATUS_UNKNOWN } from "./exit-status";
import { MediaStore, MprisStore } from "./mediaModel";
import { SurfaceStore } from "./SurfaceStore";
import { TerminalStore, type YasWasmModule } from "./TerminalStore";
import type {
  ConnectionId,
  ConnectionStatus,
  CopyRangeResult,
  SessionId,
  SurfaceId,
  TerminalPalette,
  YasConnectionSnapshot,
  YasSearchResult,
  YasSession,
  YasClientList,
} from "./types";
import {
  CODEC_SUPPORT_AV1,
  CODEC_SUPPORT_AV1_444,
  CODEC_SUPPORT_H264,
  CODEC_SUPPORT_H264_444,
  SURFACE_FRAME_CODEC_AV1,
  SURFACE_FRAME_CODEC_H264,
  SURFACE_FRAME_CODEC_PNG,
  SURFACE_FRAME_FLAG_KEYFRAME,
} from "./surfaceModel";
import { getCodecSupport } from "./YasSurfaceCanvas";
import type {
  AwaitSessionExitOptions,
  CreateSessionOptions,
  SurfaceTarget,
} from "./workspaceConnectionTypes";
import type {
  SurfaceAxisEvent,
  SurfaceDragItem,
  SurfaceTouchPoint,
} from "./input";
import {
  YAS_FAMILY_SELECTION,
  YAS_FAMILY_FONT,
  YAS_FAMILY_SURFACE,
  YAS_FAMILY_TERMINAL,
  YAS_CLASS_EVENT,
  YAS_CLASS_REQUEST,
  YAS_FONT_STATE,
  YAS_FONT_STATE_ACK,
  YAS_FONT_UNWATCH,
  YAS_FONT_WATCH,
  YAS_SELECTION_ACTION_COPY,
  YAS_SELECTION_STATE,
  YAS_SELECTION_STATE_ACK,
  YAS_SELECTION_UNWATCH,
  YAS_SELECTION_WATCH,
  YAS_SELECTION_MAX_INLINE_BYTES,
  YAS_SELECTION_OWNER_NONE,
  YAS_SELECTION_OWNER_SESSION,
  YAS_SELECTION_SLOT_CLIPBOARD,
  YAS_SELECTION_SLOT_PRIMARY,
  YAS_TERMINAL_COMMAND_ARGV,
  YAS_TERMINAL_COMMAND_DEFAULT_SHELL,
  YAS_TERMINAL_COMMAND_SHELL_COMMAND,
  YAS_TERMINAL_CREATE_RESOURCE_TAG_EXTENSION,
  YAS_TERMINAL_CUTOVER_STOP_THEN_START,
  YAS_TERMINAL_CWD_PATH,
  YAS_TERMINAL_CWD_SERVER_DEFAULT,
  YAS_TERMINAL_CWD_TERMINAL,
  YAS_TERMINAL_ENVIRONMENT_SERVER,
  YAS_TERMINAL_ENVIRONMENT_SET,
  YAS_TERMINAL_EXIT_KIND_CODE,
  YAS_TERMINAL_EXIT_KIND_SIGNAL,
  YAS_TERMINAL_FRAME_KEYFRAME,
  YAS_TERMINAL_GRID_CODEC_V1,
  YAS_TERMINAL_STATE,
  YAS_TERMINAL_STATE_ACK,
  YAS_TERMINAL_UNWATCH,
  YAS_TERMINAL_WATCH,
  YAS_TERMINAL_LAUNCH_DEADLINE_AFTER_NS_EXTENSION,
  YAS_TERMINAL_LAUNCH_REPLAY,
  YAS_TERMINAL_LIFECYCLE_EXITED,
  YAS_TERMINAL_MODIFIER_ALT,
  YAS_TERMINAL_MODIFIER_CTRL,
  YAS_TERMINAL_MODIFIER_SHIFT,
  YAS_TERMINAL_MOUSE_ACTION_DOWN,
  YAS_TERMINAL_MOUSE_ACTION_MOVE,
  YAS_TERMINAL_MOUSE_ACTION_UP,
  YAS_TERMINAL_MOUSE_BUTTON_LEFT,
  YAS_TERMINAL_MOUSE_BUTTON_MIDDLE,
  YAS_TERMINAL_MOUSE_BUTTON_NONE,
  YAS_TERMINAL_MOUSE_BUTTON_RIGHT,
  YAS_TERMINAL_QUERY_REPRESENTATION_PLAIN,
  YAS_TERMINAL_SCROLL_ABSOLUTE,
  YAS_TERMINAL_SCROLL_RELATIVE,
  YAS_TERMINAL_SIGNAL_HANGUP,
  YAS_TERMINAL_SIGNAL_INTERRUPT,
  YAS_TERMINAL_SIGNAL_KILL,
  YAS_TERMINAL_SIGNAL_TERMINATE,
  YAS_TERMINAL_WHEEL_SOURCE_WHEEL,
  YAS_SURFACE_AXIS_SOURCE_WHEEL,
  YAS_SURFACE_AXIS,
  YAS_SURFACE_CODEC_AV1_V1,
  YAS_SURFACE_CODEC_H264_V1,
  YAS_SURFACE_CODEC_PNG_V1,
  YAS_SURFACE_FRAME_END_OF_STREAM,
  YAS_SURFACE_FRAME_KEYFRAME,
  YAS_SURFACE_KEY_STATE_PRESSED,
  YAS_SURFACE_KEY_STATE_RELEASED,
  YAS_SURFACE_KEY_STATE_REPEAT,
  YAS_SURFACE_KEY,
  YAS_SURFACE_MODIFIER_ALT,
  YAS_SURFACE_MODIFIER_CONTROL,
  YAS_SURFACE_MODIFIER_SHIFT,
  YAS_SURFACE_MODIFIER_SUPER,
  YAS_SURFACE_POINTER_BUTTON_MIDDLE,
  YAS_SURFACE_POINTER_BUTTON_NONE,
  YAS_SURFACE_POINTER_BUTTON_PRIMARY,
  YAS_SURFACE_POINTER_BUTTON_SECONDARY,
  YAS_SURFACE_POINTER_PHASE_DOWN,
  YAS_SURFACE_POINTER_PHASE_LEAVE,
  YAS_SURFACE_POINTER_PHASE_MOVE,
  YAS_SURFACE_POINTER_PHASE_UP,
  YAS_SURFACE_POINTER,
  YAS_SURFACE_REMOTE_INPUT_POINTER,
  YAS_SURFACE_PREEDIT,
  YAS_SURFACE_TEXT,
  YAS_SURFACE_STATE,
  YAS_SURFACE_STATE_ACK,
  YAS_SURFACE_UNWATCH,
  YAS_SURFACE_WATCH,
  YAS_SURFACE_TOUCH_PHASE_CANCEL,
  YAS_SURFACE_TOUCH_PHASE_DOWN,
  YAS_SURFACE_TOUCH_PHASE_FRAME,
  YAS_SURFACE_TOUCH_PHASE_MOVE,
  YAS_SURFACE_TOUCH_PHASE_UP,
  YAS_SURFACE_TOUCH,
  YAS_STATUS_RESOURCE_EXHAUSTED,
  YAS_STATUS_UNAVAILABLE,
  YAS_STATUS_UNSUPPORTED,
  YAS_TERMINAL_COPY_RANGE,
} from "./yas/generated";
import * as yasGenerated from "./yas/generated";
import { YasFontClient, YasFontProtocol } from "./yas/font";
import { YasNativeDesktopClientLifecycle } from "./yas/nativeDesktopMedia";
import {
  YasNativeExtensionFacade,
  type YasNativeExtensionInstallRequest,
} from "./yas/nativeExtensionFacade";
import type { YasExtensionRecord } from "./yas/extension";
import {
  YasNativeChannelFacade,
  type YasNativeChannelHandle,
  type YasNativeChannelNamesWatch,
  type YasNativeChannelOpenOptions,
} from "./yas/nativeChannelFacade";
import { YasNativeProductFamilies } from "./yas/nativeProductFamilies";
import { decodeSurfaceCodecPayload } from "./yas/packed";
import {
  YasNativeWorkspaceFs,
  type YasNativeFsSyncHandle,
  type YasNativeFsSyncOptions,
} from "./yas/nativeWorkspaceFs";
import {
  YasNativeWorkspaceGit,
  type YasNativeGitDiscoverOptions,
  type YasNativeGitFoundRepo,
  type YasNativeGitOpenOptions,
  type YasNativeGitRepoHandle,
} from "./yas/nativeWorkspaceGit";
import {
  YasNativeWorkspaceLsp,
  type YasNativeLspHandle,
  type YasNativeLspOpenOptions,
} from "./yas/nativeWorkspaceLsp";
import { YasNativeWorkspaceKv } from "./yas/nativeWorkspaceKv";
import type { FsFileIndex, FsGrepOptions, FsGrepResult } from "./fsModel";
import type {
  WorkspaceSessionKvDeleteOptions,
  WorkspaceSessionKvPutOptions,
  WorkspaceSessionKvWatch,
  WorkspaceSessionKvWatchOptions,
} from "./workspaceSessionKv";
import {
  YasSelectionClient,
  selectionDragDropItemsExtension,
  type YasSelectionGet,
  type YasSelectionSlotRecord,
} from "./yas/selection";
import {
  YasConnection as NativeYasSession,
  type YasInvalidation,
} from "./yas/session";
import {
  YasSurfaceClient,
  surfaceActivationRevision,
  surfaceCursorState,
  surfaceResizeScale120Extension,
  surfaceTextInputState,
  type YasSurfaceFrame,
  type YasSurfaceRecord,
  type YasSurfaceView,
} from "./yas/surface";
import {
  YasTerminalClient,
  decodeTerminalGridV1,
  type YasTerminalFrameEvent,
  type YasTerminalGridState,
  type YasTerminalRecord,
  type YasTerminalViewConfiguration,
  type YasTerminalView,
} from "./yas/terminal";
import { encodeBrowserTerminalGrid } from "./yas/terminalRenderer";
import { YasResultError, YasWriter } from "./yas/wire";

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();
const MAX_QUERY_BYTES = 8 * 1024 * 1024;

/** A custom Wayland cursor expressed as a valid CSS cursor value. */
export function customSurfaceCursorCss(
  url: string,
  hotspotX: number,
  hotspotY: number,
  scale120: number,
): string {
  const bufferScale = Math.max(120, scale120) / 120;
  const logicalX = Math.max(0, Math.round(hotspotX));
  const logicalY = Math.max(0, Math.round(hotspotY));
  const rawX = Math.max(0, Math.round(hotspotX * bufferScale));
  const rawY = Math.max(0, Math.round(hotspotY * bufferScale));
  const image = `url(${JSON.stringify(url)})`;
  if (bufferScale === 1)
    return `${image} ${logicalX} ${logicalY}, default`;
  // Wayland's hotspot is surface-local (logical), while the PNG is the raw
  // cursor buffer. image-set carries that buffer density into CSS, whose
  // hotspot is then expressed in the resolved image coordinate system. Keep
  // the raw URL as a fallback for engines without cursor url-set support.
  return `image-set(${image} ${bufferScale}x) ${logicalX} ${logicalY}, ${image} ${rawX} ${rawY}, default`;
}
// One slot turns the reliable Surface stream into stop-and-wait: every frame
// has to cross the link, enter WebCodecs, and send its ACK back before the
// server may emit the next one. Worse, browser/native scheduling can batch
// otherwise-immediate decode ACKs for about 100 ms. Cover that at 120 Hz plus
// four scheduling slots. Byte credit independently bounds queued video, so
// this sequence window supplies RTT headroom for small frames without
// admitting a second oversized frame.
const NATIVE_SURFACE_DECODER_CAPACITY = 16;

interface NativeTerminalViewState {
  view: YasTerminalView;
  removeFrames: () => void;
  grids: Map<number, YasTerminalGridState>;
  pendingSequences: number[];
  lastPresented: number;
}

interface PendingTerminalView {
  promise: Promise<void>;
  cancelled: boolean;
}

interface PendingSurfaceView {
  promise: Promise<void>;
  cancelled: boolean;
}

interface ViewSize {
  rows: number;
  cols: number;
  isActive?: () => boolean;
}

interface NativeSurfaceMount {
  target: SurfaceTarget | null;
  maxFps: number;
}

interface NativeSurfaceViewState {
  view: YasSurfaceView;
  removeFrames: () => void;
  width: number;
  height: number;
  maxFps: number;
  lastReceived: bigint;
  lastPresented: bigint;
  decoderQueueDepth: number;
}

interface NativeDragItemData {
  readonly ready: Promise<NativeDragPayload | null>;
  resolve(value: NativeDragPayload | null): void;
}

interface NativeDragPayload {
  mime: string;
  name: string;
  data: Uint8Array | Blob;
}

interface NativeBrowserDrag {
  readonly token: number;
  readonly identity: Promise<{ dragHandle: bigint; revision: bigint }>;
  readonly items: readonly NativeDragItemData[];
  readonly offeredMimes: readonly (readonly string[])[];
  targetSurface: SurfaceId;
  x: number;
  y: number;
  cancelled: boolean;
}

/** Product connection backed directly by typed YAS family clients.
 *
 * This class is intentionally not a byte-transport adapter. Its resource
 * identity is the server's opaque bigint handle, and presentation codecs are
 * used only after typed family frames have been decoded.
 */
export class YasNativeWorkspaceConnection {
  readonly usesNativeSelectionDrag = true;
  readonly transport;
  readonly surfaceStore = new SurfaceStore();
  readonly audioPlayer = new AudioPlayer();
  readonly desktopStore = new DesktopStore();
  readonly mediaStore = new MediaStore();
  readonly mprisStore = new MprisStore();
  readonly native: YasNativeProductFamilies;
  private readonly workspaceFs: YasNativeWorkspaceFs;
  private readonly workspaceGit: YasNativeWorkspaceGit;
  private readonly workspaceLsp: YasNativeWorkspaceLsp;
  private readonly workspaceKv: YasNativeWorkspaceKv;
  fontProtocol: YasFontProtocol | null = null;

  private terminalClient: YasTerminalClient | null = null;
  private selectionClient: YasSelectionClient | null = null;
  private channelFacade: YasNativeChannelFacade | null = null;
  private extensionFacade: YasNativeExtensionFacade | null = null;
  private desktopMedia: YasNativeDesktopClientLifecycle | null = null;
  private surface: YasSurfaceClient | null = null;
  private readonly store: TerminalStore;
  private readonly listeners = new Set<() => void>();
  private readonly termCwdListeners = new Set<
    (sessionId: SessionId, cwd: string) => void
  >();
  private readonly sessions = new Map<SessionId, YasSession>();
  private readonly records = new Map<bigint, YasTerminalRecord>();
  private readonly views = new Map<bigint, NativeTerminalViewState>();
  private readonly viewSizes = new Map<bigint, Map<string, ViewSize>>();
  private readonly surfaceRecords = new Map<SurfaceId, YasSurfaceRecord>();
  private readonly surfaceViews = new Map<SurfaceId, NativeSurfaceViewState>();
  private readonly surfaceMounts = new Map<
    SurfaceId,
    Map<string, NativeSurfaceMount>
  >();
  private readonly surfaceViewSizes = new Map<
    SurfaceId,
    Map<string, { width: number; height: number; scale120: number }>
  >();
  private readonly pressedSurfaceKeys = new Set<number>();
  /** Latest browser display cadence measured by TerminalStore's rAF probe. */
  private displayFps = 60;
  private surfaceMaxFps = 0;
  /** Retained UI preferences; YAS Surface v1 has no bandwidth/speed knobs. */
  defaultSurfaceBandwidth = 0;
  defaultSurfaceSpeed = 0;
  defaultAudioBitrateKbps = 0;
  surfaceStreamingEnabled = true;
  private surfaceTouchUsers = 0;
  private browserDrag: NativeBrowserDrag | null = null;
  private nextBrowserDragToken = 1;
  private nextSurfaceViewToken = 1;
  private readonly scrollAnchorListeners = new Map<
    bigint,
    Set<(offset: number) => void>
  >();
  private selectionSlots: readonly YasSelectionSlotRecord[] = [];
  private removeCatalog: (() => void) | null = null;
  private removeSelectionCatalog: (() => void) | null = null;
  private removeSurfaceCatalog: (() => void) | null = null;
  private removeSurfaceRemoteInput: (() => void) | null = null;
  private removeSelectionGet: (() => void) | null = null;
  private removeSessionReady: (() => void) | null = null;
  private removeSessionInvalidation: (() => void) | null = null;
  private removeSessionCatalogChange: (() => void) | null = null;
  private removeReceiveBudgetCapacity: (() => void) | null = null;
  private familyInitializationEpoch = 0;
  private familyInitializationPending = false;
  private familyInitializationError: string | null = null;
  private familyReconfigurationNeeded = false;
  private familyInitializationQueued = false;
  private familyInitializationRunning = false;
  private familyGenerationBumpPending = false;
  private disposed = false;
  private readyListeners = new Set<() => void>();
  private nextViewToken = 1;
  private readonly pendingViews = new Map<bigint, PendingTerminalView>();
  private readonly desiredTerminalViews = new Set<bigint>();
  private terminalViewAdmissionScheduled = false;
  private terminalViewAdmissionRunning = false;
  private terminalViewAdmissionWakePending = false;
  private terminalViewAdmissionBlocker: bigint | null = null;
  private terminalViewAdmissionEpoch = 0;
  private readonly pendingSurfaceViews = new Map<
    SurfaceId,
    PendingSurfaceView
  >();
  private generation = 0;
  private focusedSessionId: SessionId | null = null;
  private snapshot: YasConnectionSnapshot;

  constructor(
    readonly id: ConnectionId,
    readonly session: NativeYasSession,
    wasm: YasWasmModule | Promise<YasWasmModule>,
    private readonly autoConnect = true,
  ) {
    this.transport = session.transport;
    this.native = new YasNativeProductFamilies(session);
    this.workspaceFs = new YasNativeWorkspaceFs(session, {
      terminalHandle: (sessionId) => {
        const terminalSession = this.sessions.get(sessionId);
        return typeof terminalSession?.ptyId === "bigint"
          ? terminalSession.ptyId
          : undefined;
      },
    });
    this.workspaceGit = new YasNativeWorkspaceGit(session, {
      terminalHandle: (sessionId) => {
        const terminalSession = this.sessions.get(sessionId);
        return typeof terminalSession?.ptyId === "bigint"
          ? terminalSession.ptyId
          : undefined;
      },
    });
    this.workspaceLsp = new YasNativeWorkspaceLsp(session, {
      terminalHandle: (sessionId) => {
        const terminalSession = this.sessions.get(sessionId);
        return typeof terminalSession?.ptyId === "bigint"
          ? terminalSession.ptyId
          : undefined;
      },
    });
    this.workspaceKv = new YasNativeWorkspaceKv(session);
    this.surfaceStore.setConnectionId(id);
    this.surfaceStore.setAckSender((surfaceId, queueDepth) =>
      this.acknowledgeSurface(surfaceId, queueDepth),
    );
    this.surfaceStore.setKeyframeSender((surfaceId) => {
      void this.surfaceViews.get(surfaceId)?.view.reset();
    });
    this.store = new TerminalStore(
      {
        getStatus: () => this.connectionStatus,
        subscribeTerminal: (terminalId) => {
          if (typeof terminalId === "bigint")
            this.subscribeTerminalView(terminalId);
        },
        unsubscribeTerminal: (terminalId) => {
          if (typeof terminalId === "bigint")
            this.unsubscribeTerminalView(terminalId);
        },
        acknowledgeTerminalFrame: (terminalId) => {
          if (typeof terminalId === "bigint") this.acknowledge(terminalId);
        },
        setTerminalDisplayRate: (fps) => this.configureDisplayRate(fps),
        // Queue depth is carried on each native FRAME_ACK. There is no second
        // connection-global terminal-metrics stream in YAS v1.
        reportTerminalMetrics: () => undefined,
      },
      wasm,
    );
    this.snapshot = this.emptySnapshot();
    this.transport.addEventListener("statuschange", this.onTransportStatus);
    this.removeSessionReady = this.session.onReady(() => this.onSessionReady());
    this.removeSessionInvalidation = this.session.onInvalidation(
      (invalidation) => this.onSessionInvalidation(invalidation),
    );
    this.removeSessionCatalogChange = this.session.onCatalogChange(() =>
      this.onSessionCatalogChange(),
    );
    this.removeReceiveBudgetCapacity =
      this.session.receiveBudget.onCapacityAvailable(() =>
        this.onReceiveBudgetCapacity(),
      );
    if (autoConnect) this.connect();
  }

  private get terminal(): YasTerminalClient {
    if (!this.terminalClient)
      throw new Error("Terminal family is not ready or was not negotiated");
    return this.terminalClient;
  }

  private get selection(): YasSelectionClient {
    if (!this.selectionClient)
      throw new Error("Selection family is not ready or was not negotiated");
    return this.selectionClient;
  }

  get processProtocol() {
    return this.native.processProtocol;
  }

  get envProtocol() {
    return this.native.envProtocol;
  }

  get eventsProtocol() {
    return this.native.eventsProtocol;
  }

  syncFs(
    path: string,
    options: YasNativeFsSyncOptions = {},
  ): Promise<YasNativeFsSyncHandle> {
    return this.workspaceFs.syncFs(path, options);
  }

  searchFiles(root: string, query: string, limit?: number): Promise<string[]> {
    return this.workspaceFs.searchFiles(root, query, limit);
  }

  indexFiles(root: string): Promise<FsFileIndex> {
    return this.workspaceFs.indexFiles(root);
  }

  /**
   * What the server runs on — `{ os: "linux", arch: "x86_64", env: "gnu" }` —
   * or null before HELLO and from a server that does not say.
   */
  serverPlatform(): YasPlatform | null {
    const hello = this.session.hello;
    return hello ? serverPlatform(hello.extensions) : null;
  }

  /** One-shot batch read by absolute path, for artwork and other small files. */
  readFiles(
    groups: readonly (readonly string[])[],
    options?: { flags?: number; maxBytes?: number },
  ): Promise<{ status: number; path: string; content: Uint8Array }[]> {
    return this.workspaceFs.readFiles(groups, options);
  }

  grep(
    root: string,
    query: string,
    options: FsGrepOptions = {},
  ): Promise<FsGrepResult> {
    return this.workspaceFs.grep(root, query, options);
  }

  openRepo(
    path: string,
    options: YasNativeGitOpenOptions = {},
  ): Promise<YasNativeGitRepoHandle> {
    return this.workspaceGit.openRepo(path, options);
  }

  discoverRepos(
    path: string,
    options: YasNativeGitDiscoverOptions = {},
  ): Promise<YasNativeGitFoundRepo[]> {
    return this.workspaceGit.discoverRepos(path, options);
  }

  openLsp(
    path: string,
    options: YasNativeLspOpenOptions = {},
  ): Promise<YasNativeLspHandle> {
    return this.workspaceLsp.openLsp(path, options);
  }

  kvPut(
    key: string,
    value: Uint8Array,
    options: WorkspaceSessionKvPutOptions = {},
  ): Promise<{ hash: Uint8Array; mtimeNs: bigint }> {
    return this.workspaceKv.kvPut(key, value, options);
  }

  kvDelete(
    key: string,
    options: WorkspaceSessionKvDeleteOptions = {},
  ): Promise<void> {
    return this.workspaceKv.kvDelete(key, options);
  }

  kvFetch(
    key: string,
  ): Promise<{ hash: Uint8Array; value: Uint8Array } | null> {
    return this.workspaceKv.kvFetch(key);
  }

  watchKv(
    prefix: string,
    options: WorkspaceSessionKvWatchOptions = {},
  ): Promise<WorkspaceSessionKvWatch> {
    return this.workspaceKv.watchKv(prefix, options);
  }

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  getSnapshot = (): YasConnectionSnapshot => this.snapshot;

  getDebugStats(sessionId: SessionId | null) {
    const session = sessionId ? this.sessions.get(sessionId) : null;
    return {
      ...this.store.getDebugStats(session?.ptyId ?? null),
      surfaces: this.surfaceStore.getDebugStats(),
      audioBuffer: {
        ...this.audioPlayer.bufferStats,
        fastPath: "native typed Media",
      },
    };
  }

  connect(): void {
    if (this.disposed) return;
    void this.session.connect().catch(() => this.refreshSnapshot());
  }

  reconnect(): void {
    if (this.disposed) return;
    if (this.transport.reconnect) this.transport.reconnect();
    else this.transport.connect();
  }

  close(): void {
    this.session.close();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.removeCatalog?.();
    this.removeSelectionCatalog?.();
    this.removeSurfaceCatalog?.();
    this.removeSurfaceRemoteInput?.();
    this.removeSelectionGet?.();
    this.removeSelectionGet = null;
    this.removeSessionReady?.();
    this.removeSessionReady = null;
    this.removeSessionInvalidation?.();
    this.removeSessionInvalidation = null;
    this.removeSessionCatalogChange?.();
    this.removeSessionCatalogChange = null;
    this.removeReceiveBudgetCapacity?.();
    this.removeReceiveBudgetCapacity = null;
    this.familyInitializationEpoch++;
    this.familyInitializationPending = false;
    this.familyReconfigurationNeeded = false;
    this.familyInitializationQueued = false;
    this.familyGenerationBumpPending = false;
    this.cancelBrowserDrag("connection disposed");
    this.transport.removeEventListener("statuschange", this.onTransportStatus);
    this.closeViewsLocal();
    this.terminalClient?.dispose();
    this.terminalClient = null;
    this.selectionClient?.dispose();
    this.selectionClient = null;
    this.surface?.dispose();
    this.channelFacade?.dispose();
    this.channelFacade = null;
    this.extensionFacade?.dispose();
    this.extensionFacade = null;
    this.desktopMedia?.dispose();
    this.desktopMedia = null;
    this.fontProtocol?.dispose();
    this.fontProtocol = null;
    this.workspaceFs.dispose();
    this.workspaceGit.dispose();
    this.workspaceLsp.dispose();
    this.workspaceKv.dispose();
    this.native.dispose();
    this.store.destroy();
    this.surfaceStore.destroy();
    this.audioPlayer.destroy();
    this.desktopStore.reset();
    this.mediaStore.reset();
    this.listeners.clear();
    this.termCwdListeners.clear();
    this.readyListeners.clear();
  }

  getSession(sessionId: SessionId): YasSession | null {
    return this.sessions.get(sessionId) ?? null;
  }

  async createSession(options: CreateSessionOptions): Promise<YasSession> {
    const source = options.cwdFromSessionId
      ? this.requireSession(options.cwdFromSessionId)
      : null;
    const command = options.argv
      ? {
          kind: YAS_TERMINAL_COMMAND_ARGV,
          argv: options.argv.map((part) => textEncoder.encode(part)),
        }
      : options.command !== undefined
        ? {
            kind: YAS_TERMINAL_COMMAND_SHELL_COMMAND,
            command: options.command,
          }
        : { kind: YAS_TERMINAL_COMMAND_DEFAULT_SHELL };
    // What the child needs to know it is talking to a terminal.
    //
    // `ENVIRONMENT_SERVER` means "start from the server's own environment",
    // and a server started by a service manager has no `TERM` in it — so
    // without this every shell opened from a browser ran as if it had no
    // terminal at all: fish drew no colours, and `tmux attach` refused with
    // "terminal does not support clear". This end is the one that knows what
    // it can render, so this end declares it. A caller that passes its own
    // `TERM` still wins.
    const environment = Object.entries({
      TERM: "xterm-256color",
      COLORTERM: "truecolor",
      ...(options.env ?? {}),
    })
      .map(([key, value]) => ({
        key: textEncoder.encode(key),
        kind: YAS_TERMINAL_ENVIRONMENT_SET,
        value: textEncoder.encode(value),
      }))
      .sort((left, right) => compareBytes(left.key, right.key));
    const launchExtensions =
      options.deadlineMs === undefined
        ? []
        : [
            {
              tag: YAS_TERMINAL_LAUNCH_DEADLINE_AFTER_NS_EXTENSION,
              value: new YasWriter()
                .u64(BigInt(options.deadlineMs) * 1_000_000n)
                .finish(),
            },
          ];
    const result = await this.terminal.create({
      rows: options.rows,
      cols: options.cols,
      operationId: operationId(),
      launch: {
        command,
        cwd: options.cwd
          ? {
              kind: YAS_TERMINAL_CWD_PATH,
              path: textEncoder.encode(options.cwd),
            }
          : source
            ? {
                kind: YAS_TERMINAL_CWD_TERMINAL,
                terminalHandle: this.handleForSession(source.id),
              }
            : { kind: YAS_TERMINAL_CWD_SERVER_DEFAULT },
        environmentBase: YAS_TERMINAL_ENVIRONMENT_SERVER,
        environment,
        extensions: launchExtensions,
      },
      extensions: options.tag
        ? [
            {
              tag: YAS_TERMINAL_CREATE_RESOURCE_TAG_EXTENSION,
              value: textEncoder.encode(options.tag),
            },
          ]
        : [],
    });
    await this.waitForRecord(result.terminalHandle);
    const created = this.sessions.get(this.sessionId(result.terminalHandle));
    if (!created) throw new Error("Terminal CREATE completed without state");
    if (options.subscribe !== false) {
      this.store.setDesiredSubscriptions(
        new Set([...this.visibleHandles(), result.terminalHandle]),
      );
    }
    return created;
  }

  awaitSessionExit(
    sessionId: SessionId,
    options: AwaitSessionExitOptions = {},
  ): Promise<YasSession> {
    if (!this.sessions.has(sessionId))
      return Promise.reject(new Error("Unknown session"));
    return new Promise((resolve, reject) => {
      let timer: ReturnType<typeof setTimeout> | undefined;
      const remove = this.subscribe(() => {
        const current = this.sessions.get(sessionId);
        if (
          !current ||
          (current.state !== "exited" && current.state !== "closed")
        )
          return;
        remove();
        if (timer) clearTimeout(timer);
        resolve(current);
      });
      if (options.timeoutMs !== undefined)
        timer = setTimeout(() => {
          remove();
          reject(new Error("Session exit wait timed out"));
        }, options.timeoutMs);
    });
  }

  async closeSession(sessionId: SessionId): Promise<void> {
    await this.terminal.close(this.handleForSession(sessionId), operationId());
  }

  restartSession(sessionId: SessionId): void {
    void this.terminal.restart(
      this.handleForSession(sessionId),
      operationId(),
      {
        launchMode: YAS_TERMINAL_LAUNCH_REPLAY,
        cutoverMode: YAS_TERMINAL_CUTOVER_STOP_THEN_START,
      },
    );
  }

  killSession(sessionId: SessionId, signal = 15): void {
    const kind =
      signal === 2
        ? YAS_TERMINAL_SIGNAL_INTERRUPT
        : signal === 9
          ? YAS_TERMINAL_SIGNAL_KILL
          : signal === 1
            ? YAS_TERMINAL_SIGNAL_HANGUP
            : YAS_TERMINAL_SIGNAL_TERMINATE;
    void this.terminal.signal(
      this.handleForSession(sessionId),
      operationId(),
      kind,
    );
  }

  focusSession(sessionId: SessionId | null): void {
    const previous = this.focusedSessionId;
    this.focusedSessionId = sessionId;
    if (previous) {
      const view = this.views.get(this.handleForSession(previous));
      if (view) void this.terminal.setFocus(view.view.result.viewId, false);
    }
    if (sessionId) {
      const view = this.views.get(this.handleForSession(sessionId));
      if (view) void this.terminal.setFocus(view.view.result.viewId, true);
    }
    this.refreshSnapshot();
  }

  sendInput(sessionId: SessionId, data: Uint8Array): void {
    const handle = this.handleForSession(sessionId);
    const state = this.views.get(handle);
    if (state) this.terminal.input(this.feedback(state), data);
    else this.terminal.write(handle, data);
  }

  resizeSession(sessionId: SessionId, rows: number, cols: number): void {
    const handle = this.handleForSession(sessionId);
    void this.terminal.resize(handle, rows, cols);
    const state = this.views.get(handle);
    if (state) this.configureView(handle, { rows, cols });
  }

  resizeSessions(
    entries: Iterable<{ sessionId: SessionId; rows: number; cols: number }>,
  ): void {
    for (const entry of entries)
      this.resizeSession(entry.sessionId, entry.rows, entry.cols);
  }

  clearSessionSize(sessionId: SessionId): void {
    this.clearSessionSizes([sessionId]);
  }

  clearSessionSizes(sessionIds: Iterable<SessionId>): void {
    for (const sessionId of sessionIds) {
      const handle = this.handleForSession(sessionId);
      this.viewSizes.delete(handle);
    }
  }

  scrollSession(sessionId: SessionId, offset: number): void {
    const state = this.views.get(this.handleForSession(sessionId));
    if (!state) return;
    void this.terminal
      .scroll(
        state.view.result.viewId,
        BigInt(offset),
        YAS_TERMINAL_SCROLL_ABSOLUTE,
      )
      .then((applied) => this.emitScrollAnchor(sessionId, Number(applied)));
  }

  scrollSessionBy(sessionId: SessionId, _offset: number, lines: number): void {
    const state = this.views.get(this.handleForSession(sessionId));
    if (!state) return;
    void this.terminal
      .scroll(
        state.view.result.viewId,
        BigInt(lines),
        YAS_TERMINAL_SCROLL_RELATIVE,
      )
      .then((applied) => this.emitScrollAnchor(sessionId, Number(applied)));
  }

  sendMouse(
    sessionId: SessionId,
    type: number,
    button: number,
    col: number,
    row: number,
  ): void {
    const state = this.views.get(this.handleForSession(sessionId));
    if (!state) return;
    this.terminal.mouse(this.feedback(state), {
      clientMonotonicNs: monotonicNs(),
      action:
        type === 0
          ? YAS_TERMINAL_MOUSE_ACTION_DOWN
          : type === 1
            ? YAS_TERMINAL_MOUSE_ACTION_UP
            : YAS_TERMINAL_MOUSE_ACTION_MOVE,
      button: xtermButton(button),
      modifiers:
        (button & 4 ? YAS_TERMINAL_MODIFIER_SHIFT : 0) |
        (button & 8 ? YAS_TERMINAL_MODIFIER_ALT : 0) |
        (button & 16 ? YAS_TERMINAL_MODIFIER_CTRL : 0),
      column: col,
      row,
    });
  }

  /**
   * One wheel notch at a cell.
   *
   * The wheel is its own family event, not a button: a report puts it in a
   * block of its own, and squeezing it through the button field cost it both
   * its direction and, in anything that reads mouse reports, its meaning.
   */
  sendWheel(
    sessionId: SessionId,
    up: boolean,
    col: number,
    row: number,
    source: number = YAS_TERMINAL_WHEEL_SOURCE_WHEEL,
  ): void {
    const state = this.views.get(this.handleForSession(sessionId));
    if (!state) return;
    this.terminal.wheel(this.feedback(state), {
      clientMonotonicNs: monotonicNs(),
      source,
      dx: 0n,
      // Fixed-point 32.32, one notch: negative is up, as it is on the wire.
      dy: up ? -(1n << 32n) : 1n << 32n,
      column: col,
      row,
    });
  }

  async search(query: string): Promise<YasSearchResult[]> {
    const result = await this.terminal.searchCatalog({
      maxResults: 256,
      query,
    });
    return result.entries.flatMap((entry) => {
      const sessionId = this.sessionId(entry.terminalHandle);
      return this.sessions.has(sessionId)
        ? [
            {
              sessionId,
              connectionId: this.id,
              score: entry.score,
              primarySource: entry.primarySource,
              matchedSources: entry.matchedSources,
              scrollOffset: Number(entry.scrollOffset),
              context: entry.context,
            },
          ]
        : [];
    });
  }

  supportsCopyRange(): boolean {
    return (
      this.supportsTerminalCatalogue() &&
      this.session.operationAdvertised(
        YAS_FAMILY_TERMINAL,
        YAS_CLASS_REQUEST,
        YAS_TERMINAL_COPY_RANGE,
      )
    );
  }

  async copyRange(
    sessionId: SessionId,
    startTail: number,
    startCol: number,
    endTail: number,
    endCol: number,
  ): Promise<CopyRangeResult> {
    const handle = this.handleForSession(sessionId);
    const record = this.records.get(handle);
    if (!record) throw new Error("Unknown session");
    const result = await this.terminal.copyRange({
      terminalHandle: handle,
      generation: record.generation,
      representation: YAS_TERMINAL_QUERY_REPRESENTATION_PLAIN,
      startRow: BigInt(startTail),
      startCol,
      endRow: BigInt(endTail),
      endCol,
      maxBytes: MAX_QUERY_BYTES,
      initialReceiveCredit: BigInt(MAX_QUERY_BYTES),
    });
    return {
      text: textDecoder.decode(await result.bytes()),
      totalLines: Number(result.totalLines ?? BigInt(record.usedRows)),
    };
  }

  async sessionCwd(sessionId: SessionId): Promise<string> {
    const handle = this.handleForSession(sessionId);
    const record = this.records.get(handle);
    if (!record) return "";
    if (record.cwd) return textDecoder.decode(record.cwd);
    const result = await this.terminal.cwd({
      terminalHandle: handle,
      generation: record.generation,
      initialReceiveCredit: BigInt(MAX_QUERY_BYTES),
    });
    return textDecoder.decode(await result.bytes());
  }

  onTermCwd(listener: (sessionId: SessionId, cwd: string) => void): () => void {
    this.termCwdListeners.add(listener);
    return () => this.termCwdListeners.delete(listener);
  }

  lastPushedCwd(sessionId: SessionId): string | null {
    const record = this.records.get(this.handleForSession(sessionId));
    return record?.cwd ? textDecoder.decode(record.cwd) : null;
  }

  /**
   * Which terminals this connection should be receiving frames for.
   *
   * The caller is a reactive effect that wakes on every event this connection
   * produces, so most calls carry the set it was already given. Those return
   * here: re-priming admissions on a keystroke is churn, and it used to be
   * load bearing only because a view waiting on receive budget was retried
   * nowhere else. Retries now follow the thing they wait on — released budget
   * (`onReceiveBudgetCapacity`) and an arriving catalogue record — so an
   * unchanged set has nothing left to do.
   */
  setVisibleSessionIds(sessionIds: Iterable<SessionId>): void {
    const handles = new Set<bigint>();
    for (const sessionId of sessionIds) {
      const session = this.sessions.get(sessionId);
      if (session && session.state !== "closed")
        handles.add(this.handleForSession(sessionId));
    }
    if (sameHandles(handles, this.desiredTerminalViews)) return;
    this.desiredTerminalViews.clear();
    for (const handle of handles) this.desiredTerminalViews.add(handle);
    if (
      this.terminalViewAdmissionBlocker !== null &&
      !handles.has(this.terminalViewAdmissionBlocker)
    )
      this.terminalViewAdmissionBlocker = null;
    this.store.setDesiredSubscriptions(handles);
    this.reconcileTerminalViewAdmissionPriority();
    this.scheduleTerminalViewAdmissions();
  }

  getTerminal(sessionId: SessionId) {
    return this.store.getTerminal(this.handleForSession(sessionId));
  }

  allocViewId(): string {
    return `native-view-${this.nextViewToken++}`;
  }

  setViewSize(
    sessionId: SessionId,
    viewId: string,
    rows: number,
    cols: number,
    isActive?: () => boolean,
  ): void {
    const handle = this.handleForSession(sessionId);
    let sizes = this.viewSizes.get(handle);
    if (!sizes) this.viewSizes.set(handle, (sizes = new Map()));
    sizes.set(viewId, { rows, cols, isActive });
    this.applyEffectiveViewSize(handle);
  }

  removeView(sessionId: SessionId, viewId: string): void {
    const handle = this.handleForSession(sessionId);
    const sizes = this.viewSizes.get(handle);
    sizes?.delete(viewId);
    if (sizes?.size === 0) this.viewSizes.delete(handle);
    this.applyEffectiveViewSize(handle);
  }

  resetViewSizes(): void {
    this.viewSizes.clear();
  }

  metricsGeneration(): number {
    return this.store.metricsGeneration;
  }

  bumpMetricsGeneration(): number {
    return ++this.store.metricsGeneration;
  }

  getRetainCount(sessionId: SessionId): number {
    return this.store.getRetainCount(this.handleForSession(sessionId));
  }

  retain(sessionId: SessionId): void {
    this.store.retain(this.handleForSession(sessionId));
  }

  release(sessionId: SessionId): void {
    this.store.release(this.handleForSession(sessionId));
  }

  addDirtyListener(sessionId: SessionId, listener: () => void): () => void {
    const handle = this.handleForSession(sessionId);
    return this.store.addDirtyListener((changed) => {
      if (changed === handle) listener();
    });
  }

  addScrollAnchorListener(
    sessionId: SessionId,
    listener: (offset: number) => void,
  ): () => void {
    const handle = this.handleForSession(sessionId);
    let listeners = this.scrollAnchorListeners.get(handle);
    if (!listeners)
      this.scrollAnchorListeners.set(handle, (listeners = new Set()));
    listeners.add(listener);
    return () => listeners?.delete(listener);
  }

  getSharedRenderer() {
    return this.store.getSharedRenderer();
  }
  setCellSize(pw: number, ph: number): void {
    this.store.setCellSize(pw, ph);
  }
  getCellSize() {
    return this.store.getCellSize();
  }
  wasmMemory() {
    return this.store.wasmMemory();
  }
  noteFrameRendered(): void {
    this.store.noteFrameRendered();
  }
  invalidateAtlas(): void {
    this.store.invalidateAtlas();
  }
  setFontFamily(value: string): void {
    this.store.setFontFamily(value);
  }
  setFontSize(value: number): void {
    this.store.setFontSize(value);
  }
  setPalette(value: TerminalPalette): void {
    this.store.setPalette(value);
  }
  isReady(): boolean {
    return this.snapshot.ready && this.store.isReady();
  }
  onReady(listener: () => void): () => void {
    if (this.isReady()) {
      listener();
      return () => undefined;
    }
    this.readyListeners.add(listener);
    return () => this.readyListeners.delete(listener);
  }

  allocSurfaceViewId(): string {
    return `native-surface-view-${this.nextSurfaceViewToken++}`;
  }

  sendSurfaceSubscribe(
    surfaceId: SurfaceId,
    viewId: string,
    target: SurfaceTarget | null = null,
    maxFps = 0,
  ): void {
    let mounts = this.surfaceMounts.get(surfaceId);
    if (!mounts) this.surfaceMounts.set(surfaceId, (mounts = new Map()));
    mounts.set(viewId, { target, maxFps });
    void this.refreshNativeSurfaceView(surfaceId);
  }

  setSurfaceViewTarget(
    surfaceId: SurfaceId,
    viewId: string,
    target: SurfaceTarget | null,
    maxFps?: number,
  ): void {
    const mounts = this.surfaceMounts.get(surfaceId);
    const previous = mounts?.get(viewId);
    if (!mounts || !previous) return;
    mounts.set(viewId, {
      target,
      maxFps: maxFps === undefined ? previous.maxFps : maxFps,
    });
    void this.refreshNativeSurfaceView(surfaceId);
  }

  refreshSurfaceSubscribe(surfaceId: SurfaceId): void {
    void this.refreshNativeSurfaceView(surfaceId, true);
  }

  sendSurfaceUnsubscribe(surfaceId: SurfaceId, viewId: string): void {
    const mounts = this.surfaceMounts.get(surfaceId);
    if (!mounts?.delete(viewId)) return;
    if (mounts.size === 0) {
      this.surfaceMounts.delete(surfaceId);
      void this.closeSurfaceView(surfaceId);
    } else {
      void this.refreshNativeSurfaceView(surfaceId);
    }
  }

  sendSurfaceResubscribe(
    surfaceId: SurfaceId,
    _bandwidth: number,
    _speed: number,
  ): void {
    // Native Surface encoders are selected by typed codec/view parameters;
    // these browser presentation hints do not change the YAS view request.
    void this.refreshNativeSurfaceView(surfaceId, true);
  }

  setSurfaceMaxFpsCap(maxFps: number): void {
    this.surfaceMaxFps = Math.max(0, Math.min(0xffff, Math.round(maxFps)));
    for (const surfaceId of this.surfaceMounts.keys())
      void this.refreshNativeSurfaceView(surfaceId);
  }

  setSurfaceStreamingEnabled(enabled: boolean): void {
    if (this.surfaceStreamingEnabled === enabled) return;
    this.surfaceStreamingEnabled = enabled;
    for (const surfaceId of this.surfaceMounts.keys()) {
      if (enabled) void this.refreshNativeSurfaceView(surfaceId);
      else void this.closeSurfaceView(surfaceId);
    }
  }

  refreshCodecSupport(): void {
    for (const surfaceId of this.surfaceMounts.keys())
      void this.closeSurfaceView(surfaceId).then(() =>
        this.refreshNativeSurfaceView(surfaceId, true),
      );
  }

  offerSurfaceViewSize(
    surfaceId: SurfaceId,
    viewId: string,
    width: number,
    height: number,
    scale120 = 0,
  ): boolean {
    let sizes = this.surfaceViewSizes.get(surfaceId);
    if (!sizes) this.surfaceViewSizes.set(surfaceId, (sizes = new Map()));
    sizes.set(viewId, { width, height, scale120 });
    const effective = effectiveSurfaceSize(sizes.values());
    if (!effective || !this.surface) return false;
    void this.surface.resize(
      surfaceId,
      operationId(),
      BigInt(effective.logicalWidth) << 32n,
      BigInt(effective.logicalHeight) << 32n,
      [surfaceResizeScale120Extension(effective.scale120)],
    );
    if (this.surfaceMounts.has(surfaceId))
      void this.refreshNativeSurfaceView(surfaceId);
    return this.session.ready;
  }

  withdrawSurfaceViewSize(surfaceId: SurfaceId, viewId: string): void {
    const sizes = this.surfaceViewSizes.get(surfaceId);
    if (!sizes?.delete(viewId)) return;
    if (sizes.size === 0) this.surfaceViewSizes.delete(surfaceId);
    else {
      const effective = effectiveSurfaceSize(sizes.values());
      if (effective && this.surface)
        void this.surface.resize(
          surfaceId,
          operationId(),
          BigInt(effective.logicalWidth) << 32n,
          BigInt(effective.logicalHeight) << 32n,
          [surfaceResizeScale120Extension(effective.scale120)],
        );
    }
    if (this.surfaceMounts.has(surfaceId))
      void this.refreshNativeSurfaceView(surfaceId);
  }

  sendSurfaceFocus(surfaceId: SurfaceId): void {
    if (!this.surface) return;
    for (const handle of this.surfaceRecords.keys())
      if (handle !== surfaceId)
        void this.surface.focus(handle, operationId(), false);
    void this.surface.focus(surfaceId, operationId(), true);
  }

  sendSurfaceClose(surfaceId: SurfaceId): void {
    void this.surface?.close(surfaceId, operationId());
  }

  sendSurfaceInput(
    surfaceId: SurfaceId,
    keycode: number,
    pressed: boolean,
    timeMs = 0,
  ): void {
    const state = this.surfaceViews.get(surfaceId);
    const keyCode = evdevToHid(keycode);
    if (
      !this.surface ||
      !state ||
      keyCode === undefined ||
      !this.supportsSurfaceEvent(YAS_SURFACE_KEY)
    )
      return;
    const repeat = pressed && this.pressedSurfaceKeys.has(keycode);
    if (pressed) this.pressedSurfaceKeys.add(keycode);
    else this.pressedSurfaceKeys.delete(keycode);
    this.surface.key(state.view, this.surfaceFeedback(state), {
      clientMonotonicNs:
        timeMs > 0 ? BigInt(Math.round(timeMs * 1_000_000)) : monotonicNs(),
      keyCode,
      state: pressed
        ? repeat
          ? YAS_SURFACE_KEY_STATE_REPEAT
          : YAS_SURFACE_KEY_STATE_PRESSED
        : YAS_SURFACE_KEY_STATE_RELEASED,
      modifiers: surfaceModifiers(this.pressedSurfaceKeys),
    });
  }

  sendSurfaceText(surfaceId: SurfaceId, text: string): void {
    const state = this.surfaceViews.get(surfaceId);
    if (this.surface && state && this.supportsSurfaceEvent(YAS_SURFACE_TEXT))
      this.surface.text(
        state.view,
        this.surfaceFeedback(state),
        monotonicNs(),
        text,
      );
  }

  sendSurfacePreedit(
    surfaceId: SurfaceId,
    text: string,
    cursorUtf16: number,
  ): void {
    const state = this.surfaceViews.get(surfaceId);
    if (
      !this.surface ||
      !state ||
      !this.supportsSurfaceEvent(YAS_SURFACE_PREEDIT)
    )
      return;
    const prefixBytes = textEncoder.encode(text.slice(0, cursorUtf16)).length;
    this.surface.preedit(state.view, {
      clientMonotonicNs: monotonicNs(),
      selectionStart: prefixBytes,
      selectionEnd: prefixBytes,
      cursor: prefixBytes,
      text,
    });
  }

  sendSurfacePointer(
    surfaceId: SurfaceId,
    type: number,
    button: number,
    x: number,
    y: number,
    timeMs = 0,
  ): void {
    const state = this.surfaceViews.get(surfaceId);
    if (
      !this.surface ||
      !state ||
      !this.supportsSurfaceEvent(YAS_SURFACE_POINTER)
    )
      return;
    const phase =
      type === 0
        ? YAS_SURFACE_POINTER_PHASE_DOWN
        : type === 1
          ? YAS_SURFACE_POINTER_PHASE_UP
          : type === 3
            ? YAS_SURFACE_POINTER_PHASE_LEAVE
            : YAS_SURFACE_POINTER_PHASE_MOVE;
    const nativeButton =
      phase === YAS_SURFACE_POINTER_PHASE_MOVE ||
      phase === YAS_SURFACE_POINTER_PHASE_LEAVE
        ? YAS_SURFACE_POINTER_BUTTON_NONE
        : button === 0
          ? YAS_SURFACE_POINTER_BUTTON_PRIMARY
          : button === 1
            ? YAS_SURFACE_POINTER_BUTTON_MIDDLE
            : YAS_SURFACE_POINTER_BUTTON_SECONDARY;
    this.surface.pointer(state.view, this.surfaceFeedback(state), {
      clientMonotonicNs:
        timeMs > 0 ? BigInt(Math.round(timeMs * 1_000_000)) : monotonicNs(),
      phase,
      button: nativeButton,
      x32_32: fixed32(x),
      y32_32: fixed32(y),
    });
  }

  get supportsSurfaceTouch(): boolean {
    return (
      this.surface !== null && this.supportsSurfaceEvent(YAS_SURFACE_TOUCH)
    );
  }

  get supportsSurfaceTextInput(): boolean {
    return (
      this.surface !== null &&
      this.supportsSurfaceEvent(YAS_SURFACE_TEXT) &&
      this.supportsSurfaceEvent(YAS_SURFACE_PREEDIT)
    );
  }

  acquireSurfaceTouch(): void {
    this.surfaceTouchUsers++;
  }

  releaseSurfaceTouch(): void {
    this.surfaceTouchUsers = Math.max(0, this.surfaceTouchUsers - 1);
  }

  sendSurfaceTouch(
    surfaceId: SurfaceId,
    phase: number,
    contacts: readonly SurfaceTouchPoint[] = [],
    timeMs = 0,
  ): void {
    const state = this.surfaceViews.get(surfaceId);
    if (
      !this.surface ||
      !state ||
      this.surfaceTouchUsers === 0 ||
      !this.supportsSurfaceEvent(YAS_SURFACE_TOUCH)
    )
      return;
    const nativePhase =
      phase === 0
        ? YAS_SURFACE_TOUCH_PHASE_DOWN
        : phase === 1
          ? YAS_SURFACE_TOUCH_PHASE_UP
          : phase === 2
            ? YAS_SURFACE_TOUCH_PHASE_MOVE
            : phase === 3
              ? YAS_SURFACE_TOUCH_PHASE_CANCEL
              : YAS_SURFACE_TOUCH_PHASE_FRAME;
    this.surface.touch(
      state.view,
      timeMs > 0 ? BigInt(Math.round(timeMs * 1_000_000)) : monotonicNs(),
      nativePhase,
      contacts.map((contact) => ({
        contactId: contact.identifier,
        x32_32: fixed32(contact.x),
        y32_32: fixed32(contact.y),
      })),
    );
  }

  sendSurfaceAxis(surfaceId: SurfaceId, axis: number, valueX100: number): void {
    this.sendSurfaceAxis2(surfaceId, {
      dx: axis === 0 ? valueX100 / 100 : 0,
      dy: axis === 0 ? 0 : valueX100 / 100,
      v120x: 0,
      v120y: 0,
      source: 0,
      stop: false,
    });
  }

  sendSurfaceAxis2(surfaceId: SurfaceId, event: SurfaceAxisEvent): void {
    const state = this.surfaceViews.get(surfaceId);
    if (!this.surface || !state || !this.supportsSurfaceEvent(YAS_SURFACE_AXIS))
      return;
    this.surface.axis(state.view, this.surfaceFeedback(state), {
      clientMonotonicNs:
        event.timeMs !== undefined
          ? BigInt(Math.round(event.timeMs * 1_000_000))
          : monotonicNs(),
      source: event.source ?? YAS_SURFACE_AXIS_SOURCE_WHEEL,
      flags: (event.source === null ? 0 : 4) | (event.stop ? 8 : 0),
      dx32_32: fixed32(event.dx),
      dy32_32: fixed32(event.dy),
      stepsX: Math.round(event.v120x),
      stepsY: Math.round(event.v120y),
    });
  }

  sendSurfaceDragEnter(
    surfaceId: SurfaceId,
    x: number,
    y: number,
    mimes: string[],
    itemMimes?: string[],
  ): void {
    this.cancelBrowserDrag("replaced by a new browser drag");
    const commonMimes = orderedMimes(mimes);
    const offers =
      itemMimes && itemMimes.length > 0
        ? itemMimes.map((mime) => orderedMimes([...commonMimes, mime]))
        : [commonMimes];
    if (offers.some((mimeTypes) => mimeTypes.length === 0)) return;
    const data = offers.map(() => deferredDragItem());
    const token = this.nextBrowserDragToken++;
    const identity = this.selection.dragBegin(
      operationId(),
      YAS_SELECTION_ACTION_COPY,
      offers.map((mimeTypes) => ({ name: "", mimeTypes })),
    );
    const drag: NativeBrowserDrag = {
      token,
      identity,
      items: data,
      offeredMimes: offers,
      targetSurface: surfaceId,
      x,
      y,
      cancelled: false,
    };
    this.browserDrag = drag;
    void identity.then(
      ({ dragHandle, revision }) => {
        if (this.browserDrag !== drag || drag.cancelled) {
          void this.selection.dragCancel(
            dragHandle,
            revision,
            operationId(),
            "browser drag superseded",
          );
          return;
        }
        this.selection.dragEnter({
          dragHandle,
          revision,
          targetSurface: surfaceId,
          x32_32: fixed32(x),
          y32_32: fixed32(y),
          actions: YAS_SELECTION_ACTION_COPY,
        });
      },
      () => {
        if (this.browserDrag === drag) this.browserDrag = null;
        for (const item of data) item.resolve(null);
      },
    );
  }

  sendSurfaceDragMotion(surfaceId: SurfaceId, x: number, y: number): void {
    const drag = this.browserDrag;
    if (!drag || drag.cancelled) return;
    drag.targetSurface = surfaceId;
    drag.x = x;
    drag.y = y;
    void drag.identity.then(({ dragHandle, revision }) => {
      if (this.browserDrag !== drag || drag.cancelled) return;
      this.selection.dragMotion({
        dragHandle,
        revision,
        targetSurface: surfaceId,
        x32_32: fixed32(x),
        y32_32: fixed32(y),
        actions: YAS_SELECTION_ACTION_COPY,
      });
    });
  }

  sendSurfaceDragLeave(surfaceId: SurfaceId): void {
    const drag = this.browserDrag;
    if (!drag || drag.cancelled) return;
    void drag.identity.then(({ dragHandle, revision }) => {
      if (this.browserDrag !== drag || drag.cancelled) return;
      this.selection.dragLeave({
        dragHandle,
        revision,
        targetSurface: surfaceId,
      });
    });
  }

  sendSurfaceDragDrop(
    surfaceId: SurfaceId,
    x: number,
    y: number,
    items: SurfaceDragItem[],
  ): void {
    this.completeBrowserDrag(surfaceId, x, y, items);
  }

  /** File-backed native drag payloads remain Blob-backed until the selected
   * MIME is requested. The Selection family then chooses inline or Transfer. */
  sendSurfaceDragDropFiles(
    surfaceId: SurfaceId,
    x: number,
    y: number,
    items: Array<{ mime: string; name: string; data: Blob }>,
  ): void {
    this.completeBrowserDrag(surfaceId, x, y, items);
  }

  private completeBrowserDrag(
    surfaceId: SurfaceId,
    x: number,
    y: number,
    items: NativeDragPayload[],
  ): void {
    const drag = this.browserDrag;
    if (!drag || drag.cancelled) return;
    if (items.length !== drag.items.length || items.length === 0) {
      this.cancelBrowserDrag("browser drag item count changed");
      return;
    }
    drag.targetSurface = surfaceId;
    drag.x = x;
    drag.y = y;
    for (let index = 0; index < items.length; index++)
      drag.items[index]!.resolve(items[index]!);
    void drag.identity
      .then(async ({ dragHandle, revision }) => {
        if (this.browserDrag !== drag || drag.cancelled) return;
        this.selection.dragMotion({
          dragHandle,
          revision,
          targetSurface: surfaceId,
          x32_32: fixed32(x),
          y32_32: fixed32(y),
          actions: YAS_SELECTION_ACTION_COPY,
        });
        await this.selection.dragDrop(
          dragHandle,
          revision,
          operationId(),
          YAS_SELECTION_ACTION_COPY,
          [
            selectionDragDropItemsExtension(
              items.map((item, index) => {
                const offered = drag.offeredMimes[index]!;
                const selectedMime = offered.includes(item.mime)
                  ? item.mime
                  : offered.includes("application/octet-stream")
                    ? "application/octet-stream"
                    : offered[0]!;
                return { name: item.name, selectedMime };
              }),
            ),
          ],
        );
        if (this.browserDrag === drag) this.browserDrag = null;
      })
      .catch(() => this.cancelBrowserDrag("browser drop failed"));
  }

  sendSurfaceDragCancel(): void {
    this.cancelBrowserDrag("browser drag cancelled");
  }

  /** Direct typed Selection write. Large values use the family's bounded
   * Transfer lifecycle rather than an ad hoc browser fragmenter. */
  sendClipboard(mimeType: string, data: Uint8Array): void {
    void this.setSelection(YAS_SELECTION_SLOT_CLIPBOARD, mimeType, data);
  }

  sendPrimary(mimeType: string, data: Uint8Array): void {
    void this.setSelection(YAS_SELECTION_SLOT_PRIMARY, mimeType, data);
  }

  usesWaylandClipboard(): boolean {
    return this.selectionSlots.some(
      (slot) =>
        slot.slot === YAS_SELECTION_SLOT_CLIPBOARD &&
        slot.ownerKind !== YAS_SELECTION_OWNER_NONE &&
        slot.ownerKind !== YAS_SELECTION_OWNER_SESSION,
    );
  }

  async readWaylandClipboardText(): Promise<string | null> {
    const slot = this.selectionSlots.find(
      (candidate) => candidate.slot === YAS_SELECTION_SLOT_CLIPBOARD,
    );
    if (!slot) return null;
    const mime =
      slot.mimeTypes.find((value) => value === "text/plain;charset=utf-8") ??
      slot.mimeTypes.find((value) => value.startsWith("text/plain"));
    if (!mime) return null;
    const result = await this.selection.get({
      target: { kind: "slot", slot: slot.slot, revision: slot.revision },
      mime,
    });
    return textDecoder.decode(await result.bytes());
  }

  noteBrowserClipboardMayHaveChanged(): void {}

  subscribeClients(
    listener: (catalog: YasClientList) => void,
    onError?: (error: Error) => void,
  ): () => void {
    if (!this.desktopMedia)
      throw new Error("Client family presentation is not initialized");
    return this.desktopMedia.subscribeClients(listener, onError);
  }

  listClients(): Promise<YasClientList> {
    if (!this.desktopMedia)
      return Promise.reject(
        new Error("Client family presentation is not initialized"),
      );
    return this.desktopMedia.listClients();
  }

  kickClient(id: string, reason: string): Promise<void> {
    if (!this.desktopMedia)
      return Promise.reject(
        new Error("Client family presentation is not initialized"),
      );
    return this.desktopMedia.kickClient(id, reason);
  }

  sendAudioSubscribe(bitrateKbps = this.defaultAudioBitrateKbps): void {
    this.desktopMedia?.sendAudioSubscribe(bitrateKbps);
  }

  sendAudioUnsubscribe(): void {
    this.desktopMedia?.sendAudioUnsubscribe();
  }

  connectChannel(
    name: string,
    options: YasNativeChannelOpenOptions = {},
  ): Promise<YasNativeChannelHandle> {
    return this.requireChannelFacade().connectChannel(name, options);
  }

  watchChannelNames(
    names: readonly string[],
    onNames: (present: ReadonlySet<string>) => void,
  ): Promise<YasNativeChannelNamesWatch> {
    return this.requireChannelFacade().watchChannelNames(names, onNames);
  }

  listExtensions(): Promise<readonly YasExtensionRecord[]> {
    return this.requireExtensionFacade().listExtensions();
  }

  installExtension(
    request: YasNativeExtensionInstallRequest,
  ): Promise<YasExtensionRecord> {
    return this.requireExtensionFacade().installExtension(request);
  }

  controlExtension(
    extensionHandle: bigint,
    action: number,
  ): Promise<YasExtensionRecord | null> {
    return this.requireExtensionFacade().controlExtension(
      extensionHandle,
      action,
    );
  }

  private get connectionStatus(): ConnectionStatus {
    if (this.familyInitializationError !== null) return "error";
    if (this.familyInitializationPending) return "authenticating";
    if (this.session.ready) return "connected";
    if (this.transport.status === "connected" && this.session.hello === null)
      return "authenticating";
    return this.transport.status;
  }

  private requireChannelFacade(): YasNativeChannelFacade {
    if (this.channelFacade) return this.channelFacade;
    const client = this.native.channel;
    if (!client) throw new Error("Channel family unavailable");
    return (this.channelFacade = new YasNativeChannelFacade(
      this.session,
      client,
    ));
  }

  private requireExtensionFacade(): YasNativeExtensionFacade {
    if (this.extensionFacade) return this.extensionFacade;
    const client = this.native.extension;
    if (!client) throw new Error("Extension family unavailable");
    return (this.extensionFacade = new YasNativeExtensionFacade(
      this.session,
      client,
    ));
  }

  private readonly onTransportStatus = (_status: ConnectionStatus): void => {
    this.store.handleStatusChange(this.connectionStatus);
    this.refreshSnapshot();
  };

  private onSessionReady(): void {
    this.familyReconfigurationNeeded = false;
    this.scheduleFamilyInitialization(true);
  }

  private onSessionCatalogChange(): void {
    const bumpGeneration = this.familyReconfigurationNeeded;
    this.familyReconfigurationNeeded = false;
    this.scheduleFamilyInitialization(bumpGeneration);
  }

  private scheduleFamilyInitialization(bumpGeneration: boolean): void {
    if (this.disposed || !this.session.ready) return;
    // Catalogue updates can arrive while a WATCH is still pending. Mark the
    // current run stale and coalesce all updates behind it so only one run can
    // own wire subscriptions at a time.
    this.familyInitializationEpoch++;
    this.familyInitializationQueued = true;
    this.familyGenerationBumpPending ||= bumpGeneration;
    this.familyInitializationPending = true;
    this.familyInitializationError = null;
    this.publishFamilyInitializationState();
    if (this.familyInitializationRunning) return;
    this.familyInitializationRunning = true;
    void this.drainFamilyInitializations();
  }

  private async drainFamilyInitializations(): Promise<void> {
    try {
      while (
        this.familyInitializationQueued &&
        !this.disposed &&
        this.session.ready
      ) {
        this.familyInitializationQueued = false;
        const epoch = this.familyInitializationEpoch;
        const bumpGeneration = this.familyGenerationBumpPending;
        this.familyGenerationBumpPending = false;
        try {
          await this.initializeFamilies(epoch, bumpGeneration);
          if (!this.isCurrentFamilyInitialization(epoch)) continue;
          this.familyInitializationPending = false;
          this.familyInitializationError = null;
          this.store.handleStatusChange("connected");
          this.reconcileTerminalViewAdmissionPriority();
          this.scheduleTerminalViewAdmissions();
          this.refreshSnapshot();
        } catch (error: unknown) {
          if (!this.isCurrentFamilyInitialization(epoch)) continue;
          this.failFamilyInitialization(error);
          return;
        }
      }
    } finally {
      this.familyInitializationRunning = false;
      if (
        this.familyInitializationQueued &&
        !this.disposed &&
        this.session.ready
      ) {
        this.familyInitializationRunning = true;
        void this.drainFamilyInitializations();
      }
    }
  }

  private failFamilyInitialization(error: unknown): void {
    this.familyInitializationPending = false;
    this.familyInitializationError =
      error instanceof Error ? error.message : String(error);
    this.store.handleStatusChange("error");
    // Capability derivation can itself be the failure. Publish the lifecycle
    // error without invoking the same descriptor/native getters again.
    this.publishFamilyInitializationState();
    const reportError = (
      globalThis as typeof globalThis & {
        reportError?: (error: unknown) => void;
      }
    ).reportError;
    if (reportError) reportError(error);
    else console.error("YAS family initialization failed", error);
  }

  private publishFamilyInitializationState(): void {
    this.snapshot = {
      ...this.snapshot,
      status: this.connectionStatus,
      ready: false,
      generation: this.generation,
      error: this.familyInitializationError ?? this.transport.lastError,
    };
    this.emit();
  }

  private onSessionInvalidation(invalidation: YasInvalidation): void {
    if (invalidation.family !== undefined) {
      // The updated descriptor set is applied before the invalidation. A
      // single following catalog-change callback reconfigures all affected
      // clients without treating a live FAMILY_UPDATE as a physical failure.
      this.familyReconfigurationNeeded = true;
      this.refreshSnapshot();
      return;
    }
    this.familyInitializationEpoch++;
    this.familyInitializationPending = false;
    this.familyInitializationError = null;
    this.familyReconfigurationNeeded = false;
    this.familyInitializationQueued = false;
    this.familyGenerationBumpPending = false;
    this.desktopMedia?.dispose();
    this.desktopMedia = null;
    this.closeViewsLocal();
    this.store.handleStatusChange(this.connectionStatus);
    this.refreshSnapshot();
  }

  private isCurrentFamilyInitialization(epoch: number): boolean {
    return (
      !this.disposed &&
      epoch === this.familyInitializationEpoch &&
      this.session.ready
    );
  }

  private supportsStateCatalogue(
    family: number,
    watch: number,
    unwatch: number,
    state: number,
    stateAck: number,
  ): boolean {
    try {
      this.session.family(family);
    } catch (error) {
      if (
        error instanceof YasResultError &&
        (error.status === YAS_STATUS_UNSUPPORTED ||
          error.status === YAS_STATUS_UNAVAILABLE)
      )
        return false;
      throw error;
    }
    return (
      this.session.operationAdvertised(family, YAS_CLASS_REQUEST, watch) &&
      this.session.operationAdvertised(family, YAS_CLASS_REQUEST, unwatch) &&
      this.session.operationAdvertised(family, YAS_CLASS_EVENT, state, true) &&
      this.session.operationAdvertised(family, YAS_CLASS_EVENT, stateAck)
    );
  }

  private supportsTerminalCatalogue(): boolean {
    return this.supportsStateCatalogue(
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_WATCH,
      YAS_TERMINAL_UNWATCH,
      YAS_TERMINAL_STATE,
      YAS_TERMINAL_STATE_ACK,
    );
  }

  private supportsSurfaceCatalogue(): boolean {
    return this.supportsStateCatalogue(
      YAS_FAMILY_SURFACE,
      YAS_SURFACE_WATCH,
      YAS_SURFACE_UNWATCH,
      YAS_SURFACE_STATE,
      YAS_SURFACE_STATE_ACK,
    );
  }

  private supportsSurfaceEvent(kind: number): boolean {
    return this.session.operationAdvertised(
      YAS_FAMILY_SURFACE,
      YAS_CLASS_EVENT,
      kind,
    );
  }

  private async initializeFamilies(
    epoch: number,
    bumpGeneration: boolean,
  ): Promise<void> {
    if (!this.isCurrentFamilyInitialization(epoch)) return;
    if (bumpGeneration) this.generation++;
    this.removeCatalog?.();
    this.removeCatalog = null;
    if (this.supportsTerminalCatalogue()) {
      const terminal = (this.terminalClient ??= new YasTerminalClient(
        this.session,
      ));
      this.removeCatalog = terminal.catalog.subscribe((catalog) =>
        this.applyTerminalCatalog(catalog.terminals),
      );
      await terminal.catalog.watch();
      if (!this.isCurrentFamilyInitialization(epoch)) {
        await terminal.catalog.unwatch().catch(() => undefined);
        return;
      }
    } else if (this.terminalClient) {
      this.terminalClient.dispose();
      this.terminalClient = null;
      this.records.clear();
      this.sessions.clear();
    }
    if (this.supportsSurfaceCatalogue()) {
      const surface = (this.surface ??= new YasSurfaceClient(this.session));
      this.removeSurfaceCatalog?.();
      this.removeSurfaceCatalog = surface.catalog.subscribe((catalog) =>
        this.applySurfaceCatalog(catalog.surfaces),
      );
      this.removeSurfaceRemoteInput?.();
      this.removeSurfaceRemoteInput = surface.onRemoteInput((event) => {
        this.surfaceStore.handleRemoteInput(
          event.surfaceHandle,
          event.inputKind === YAS_SURFACE_REMOTE_INPUT_POINTER
            ? "pointer"
            : "touch",
          event.contacts.map((contact) => ({
            x: Number(contact.x32_32 >> 32n),
            y: Number(contact.y32_32 >> 32n),
          })),
        );
      });
      await surface.catalog.watch();
      if (!this.isCurrentFamilyInitialization(epoch)) {
        await surface.catalog.unwatch().catch(() => undefined);
        return;
      }
    } else if (this.surface) {
      this.removeSurfaceCatalog?.();
      this.removeSurfaceCatalog = null;
      this.removeSurfaceRemoteInput?.();
      this.removeSurfaceRemoteInput = null;
      this.surface.dispose();
      this.surface = null;
      for (const state of this.surfaceViews.values()) {
        state.removeFrames();
        state.view.closeLocal();
      }
      this.surfaceViews.clear();
      for (const handle of this.surfaceRecords.keys())
        this.surfaceStore.handleSurfaceDestroyed(handle);
      this.surfaceRecords.clear();
    }
    if (
      this.supportsStateCatalogue(
        YAS_FAMILY_SELECTION,
        YAS_SELECTION_WATCH,
        YAS_SELECTION_UNWATCH,
        YAS_SELECTION_STATE,
        YAS_SELECTION_STATE_ACK,
      )
    ) {
      const selection = (this.selectionClient ??= new YasSelectionClient(
        this.session,
      ));
      this.removeSelectionGet ??= selection.handleGet((request) =>
        this.browserDragContent(request),
      );
      this.removeSelectionCatalog?.();
      this.removeSelectionCatalog = selection.catalog.subscribe((snapshot) => {
        this.selectionSlots = snapshot.slots;
        this.emit();
      });
      await selection.catalog.watch();
      if (!this.isCurrentFamilyInitialization(epoch)) {
        await selection.catalog.unwatch().catch(() => undefined);
        return;
      }
    } else if (this.selectionClient) {
      this.removeSelectionCatalog?.();
      this.removeSelectionCatalog = null;
      this.removeSelectionGet?.();
      this.removeSelectionGet = null;
      this.selectionClient.dispose();
      this.selectionClient = null;
      this.selectionSlots = [];
    }
    if (
      this.supportsStateCatalogue(
        YAS_FAMILY_FONT,
        YAS_FONT_WATCH,
        YAS_FONT_UNWATCH,
        YAS_FONT_STATE,
        YAS_FONT_STATE_ACK,
      )
    ) {
      this.fontProtocol ??= new YasFontProtocol(
        new YasFontClient(this.session),
      );
    } else if (this.fontProtocol) {
      this.fontProtocol.dispose();
      this.fontProtocol = null;
    }
    this.desktopMedia?.dispose();
    const desktopMedia = new YasNativeDesktopClientLifecycle({
      session: this.session,
      desktopStore: this.desktopStore,
      mediaStore: this.mediaStore,
      mprisStore: this.mprisStore,
      audioPlayer: this.audioPlayer,
      onChanged: () => this.refreshSnapshot(),
    });
    this.desktopMedia = desktopMedia;
    try {
      await desktopMedia.start();
    } catch (error) {
      desktopMedia.dispose();
      if (this.desktopMedia === desktopMedia) this.desktopMedia = null;
      throw error;
    }
    if (!this.isCurrentFamilyInitialization(epoch)) {
      desktopMedia.dispose();
      if (this.desktopMedia === desktopMedia) this.desktopMedia = null;
    }
  }

  private applyTerminalCatalog(records: readonly YasTerminalRecord[]): void {
    const live = new Set(records.map((record) => record.handle));
    for (const [handle, previous] of this.records) {
      if (live.has(handle)) continue;
      this.records.delete(handle);
      const id = this.sessionId(handle);
      const session = this.sessions.get(id);
      if (session) this.sessions.set(id, { ...session, state: "closed" });
      this.store.freeTerminal(handle);
      void this.closeView(handle);
      if (previous.cwd)
        for (const listener of this.termCwdListeners)
          listener(id, textDecoder.decode(previous.cwd));
    }
    for (const record of records) {
      const previous = this.records.get(record.handle);
      this.records.set(record.handle, record);
      const id = this.sessionId(record.handle);
      this.sessions.set(id, this.publicSession(record));
      if (
        record.cwd &&
        (!previous?.cwd || !sameBytes(previous.cwd, record.cwd))
      ) {
        const cwd = textDecoder.decode(record.cwd);
        for (const listener of this.termCwdListeners) listener(id, cwd);
      }
      // A handle nobody had a record for was skipped by admission; its record
      // arriving is what makes it admissible, and is therefore where the retry
      // belongs. This used to be covered by callers re-priming admissions on
      // every event, which meant a keystroke re-primed them too.
      if (!previous && this.desiredTerminalViews.has(record.handle))
        this.scheduleTerminalViewAdmissions();
    }
    this.refreshSnapshot();
  }

  private applySurfaceCatalog(records: readonly YasSurfaceRecord[]): void {
    const live = new Set(records.map((record) => record.surfaceHandle));
    for (const handle of this.surfaceRecords.keys()) {
      if (live.has(handle)) continue;
      this.surfaceRecords.delete(handle);
      this.surfaceStore.handleSurfaceDestroyed(handle);
      void this.closeSurfaceView(handle);
    }
    for (const record of records) {
      const previous = this.surfaceRecords.get(record.surfaceHandle);
      this.surfaceRecords.set(record.surfaceHandle, record);
      const logicalWidth = Math.max(
        1,
        fixed32Integer(record.logicalWidth32_32),
      );
      const logicalHeight = Math.max(
        1,
        fixed32Integer(record.logicalHeight32_32),
      );
      // PointerMotion is encoded in exact physical composite pixels and the
      // compositor converts it back to logical coordinates. Rounded HiDPI
      // buffers cannot be reconstructed from an integer scale.
      const width = record.compositeWidth;
      const height = record.compositeHeight;
      if (!previous) {
        this.surfaceStore.handleSurfaceCreated(
          record.surfaceHandle,
          record.parentHandle,
          width,
          height,
          record.title,
          record.applicationId,
        );
        // Unlike the legacy compositor event, the native create catalogue
        // already carries both halves of the size. Publish the logical half
        // immediately as well; presentation sizing and cursor artwork must
        // not spend the surface's entire lifetime waiting for a later resize
        // that may never happen.
        this.surfaceStore.handleSurfaceResized(
          record.surfaceHandle,
          width,
          height,
          logicalWidth,
          logicalHeight,
        );
      } else {
        if (previous.title !== record.title)
          this.surfaceStore.handleSurfaceTitle(
            record.surfaceHandle,
            record.title,
          );
        if (previous.applicationId !== record.applicationId)
          this.surfaceStore.handleSurfaceAppId(
            record.surfaceHandle,
            record.applicationId,
          );
        if (
          previous.logicalWidth32_32 !== record.logicalWidth32_32 ||
          previous.logicalHeight32_32 !== record.logicalHeight32_32 ||
          previous.compositeWidth !== record.compositeWidth ||
          previous.compositeHeight !== record.compositeHeight
        )
          this.surfaceStore.handleSurfaceResized(
            record.surfaceHandle,
            width,
            height,
            logicalWidth,
            logicalHeight,
          );
      }
      const activation = surfaceActivationRevision(record);
      const oldActivation = previous
        ? surfaceActivationRevision(previous)
        : undefined;
      if (activation !== undefined && activation !== oldActivation)
        this.surfaceStore.handleSurfaceActivated(record.surfaceHandle);
      const cursor = surfaceCursorState(record);
      if (cursor) this.applySurfaceCursor(record.surfaceHandle, cursor);
      const input = surfaceTextInputState(record);
      if (input)
        this.surfaceStore.handleSurfaceTextInput(record.surfaceHandle, {
          enabled: input.enabled,
          requested: input.requested,
          hint: input.contentHint,
          purpose: input.contentPurpose,
          cursorRect: input.cursorRect,
        });
    }
    this.emit();
  }

  private applySurfaceCursor(
    surfaceId: SurfaceId,
    cursor: ReturnType<typeof surfaceCursorState> & {},
  ): void {
    if (cursor.kind === "named") {
      this.surfaceStore.handleSurfaceCursor(surfaceId, cursor.name);
    } else if (cursor.kind === "hidden") {
      this.surfaceStore.handleSurfaceCursor(surfaceId, "none", {
        kind: "hidden",
      });
    } else {
      const blob = new Blob([new Uint8Array(cursor.png)], {
        type: "image/png",
      });
      const url = URL.createObjectURL(blob);
      this.surfaceStore.handleSurfaceCursor(
        surfaceId,
        customSurfaceCursorCss(
          url,
          cursor.hotspotX,
          cursor.hotspotY,
          cursor.scale120,
        ),
        {
          kind: "custom",
          url,
          hotspotX: cursor.hotspotX,
          hotspotY: cursor.hotspotY,
          width: cursor.width,
          height: cursor.height,
          scale120: cursor.scale120,
        },
      );
    }
  }

  private async refreshNativeSurfaceView(
    surfaceId: SurfaceId,
    forceReset = false,
  ): Promise<void> {
    const pending = this.pendingSurfaceViews.get(surfaceId);
    if (pending) {
      try {
        await pending.promise;
      } catch (error) {
        if (!pending.cancelled) throw error;
      }
      // Mount geometry can change while OPEN_VIEW is in flight. Re-evaluate
      // it once the shared request settles instead of opening a second view.
      await this.refreshNativeSurfaceView(surfaceId, forceReset);
      return;
    }
    const entry: PendingSurfaceView = {
      promise: Promise.resolve(),
      cancelled: false,
    };
    entry.promise = this.openOrRefreshNativeSurfaceView(
      surfaceId,
      forceReset,
      entry,
    );
    this.pendingSurfaceViews.set(surfaceId, entry);
    void entry.promise.then(
      () => {
        if (this.pendingSurfaceViews.get(surfaceId) === entry)
          this.pendingSurfaceViews.delete(surfaceId);
      },
      () => {
        if (this.pendingSurfaceViews.get(surfaceId) === entry)
          this.pendingSurfaceViews.delete(surfaceId);
      },
    );
    await entry.promise;
  }

  private async openOrRefreshNativeSurfaceView(
    surfaceId: SurfaceId,
    forceReset: boolean,
    pending: PendingSurfaceView,
  ): Promise<void> {
    if (
      !this.surface ||
      !this.surfaceStreamingEnabled ||
      !this.session.ready ||
      !this.surfaceRecords.has(surfaceId)
    )
      return;
    const mounts = this.surfaceMounts.get(surfaceId);
    if (!mounts || mounts.size === 0) return;
    const record = this.surfaceRecords.get(surfaceId)!;
    const requestedSize = effectiveSurfaceSize(
      this.surfaceViewSizes.get(surfaceId)?.values() ?? [],
    );
    const parameters = effectiveSurfaceMount(
      mounts.values(),
      requestedSize?.physicalWidth ??
        Math.max(1, fixed32Integer(record.logicalWidth32_32)),
      requestedSize?.physicalHeight ??
        Math.max(1, fixed32Integer(record.logicalHeight32_32)),
      this.displayFps,
      this.surfaceMaxFps,
      this.surface.limits.maxViewDimension,
      this.surface.limits.maxViewPixels,
      this.surface.limits.maxFrameRate,
    );
    const existing = this.surfaceViews.get(surfaceId);
    if (existing) {
      const configurationChanged =
        existing.width !== parameters.width ||
        existing.height !== parameters.height ||
        existing.maxFps !== parameters.maxFps;
      if (configurationChanged) {
        try {
          await existing.view.configure({
            width: parameters.width,
            height: parameters.height,
            maxFps: parameters.maxFps,
            decoderCapacity: NATIVE_SURFACE_DECODER_CAPACITY,
            latencyTargetNs: 0n,
          });
        } catch (error) {
          if (!pending.cancelled) throw error;
          return;
        }
        // Publish only after CONFIGURE succeeds. A rejected reconfiguration
        // leaves the remote view unchanged and must remain retryable.
        existing.width = parameters.width;
        existing.height = parameters.height;
        existing.maxFps = parameters.maxFps;
      }
      if (
        pending.cancelled ||
        !this.surfaceStreamingEnabled ||
        this.surfaceViews.get(surfaceId) !== existing
      )
        return;
      // CONFIGURE already invalidates the encoder and requests a keyframe.
      // RESET is only useful when the configuration itself was unchanged.
      if (forceReset && !configurationChanged) {
        try {
          await existing.view.reset();
        } catch (error) {
          if (!pending.cancelled) throw error;
        }
      }
      return;
    }
    const surface = this.surface;
    const codecVersions = nativeSurfaceCodecs(getCodecSupport());
    const view = await surface.openView({
      surfaceHandle: surfaceId,
      width: parameters.width,
      height: parameters.height,
      maxFps: parameters.maxFps,
      decoderCapacity: NATIVE_SURFACE_DECODER_CAPACITY,
      codecVersions: [...codecVersions],
    });
    if (
      pending.cancelled ||
      this.disposed ||
      !this.surfaceStreamingEnabled ||
      !this.session.ready ||
      this.surface !== surface ||
      !this.surfaceRecords.has(surfaceId) ||
      !this.surfaceMounts.has(surfaceId) ||
      this.surfaceViews.has(surfaceId) ||
      !nativeSurfaceCodecs(getCodecSupport()).includes(view.result.codecVersion)
    ) {
      await view.close();
      return;
    }
    const state: NativeSurfaceViewState = {
      view,
      removeFrames: () => undefined,
      width: parameters.width,
      height: parameters.height,
      maxFps: parameters.maxFps,
      lastReceived: view.result.firstSequence - 1n,
      lastPresented: view.result.firstSequence - 1n,
      decoderQueueDepth: 0,
    };
    state.removeFrames = view.subscribe((frame) =>
      this.acceptSurfaceFrame(surfaceId, state, frame),
    );
    this.surfaceViews.set(surfaceId, state);
    this.surfaceStore.handleSurfaceEncoder(
      surfaceId,
      surfaceCodecName(view.result.codecVersion),
    );
  }

  private acceptSurfaceFrame(
    surfaceId: SurfaceId,
    state: NativeSurfaceViewState,
    frame: YasSurfaceFrame,
  ): void {
    if (frame.viewId !== state.view.result.viewId) return;
    state.lastReceived = frame.sequence;
    // EOS carries only the packed-codec metadata envelope. It is a reliable
    // lifetime boundary, not an empty access unit for WebCodecs to validate.
    if (frame.flags & YAS_SURFACE_FRAME_END_OF_STREAM) return;
    const timestampNs = frame.presentationNs || frame.captureNs;
    const timestampMs = Number((timestampNs / 1_000_000n) & 0xffff_ffffn);
    const timestampSubUs = Number((timestampNs / 1_000n) % 1_000n);
    const codec =
      frame.codecVersion === YAS_SURFACE_CODEC_H264_V1
        ? SURFACE_FRAME_CODEC_H264
        : frame.codecVersion === YAS_SURFACE_CODEC_AV1_V1
          ? SURFACE_FRAME_CODEC_AV1
          : SURFACE_FRAME_CODEC_PNG;
    const flags =
      codec |
      (frame.flags & YAS_SURFACE_FRAME_KEYFRAME
        ? SURFACE_FRAME_FLAG_KEYFRAME
        : 0);
    const bitstream = decodeSurfaceCodecPayload(
      frame.codecVersion,
      frame.payload,
    ).bitstream;
    this.surfaceStore.handleSurfaceFrame(
      surfaceId,
      timestampMs,
      flags,
      state.width,
      state.height,
      bitstream,
      timestampSubUs,
    );
  }

  private acknowledgeSurface(
    surfaceId: SurfaceId,
    decoderQueueDepth: number,
  ): void {
    const state = this.surfaceViews.get(surfaceId);
    if (!state || state.lastReceived < state.view.result.firstSequence) return;
    state.decoderQueueDepth = Math.max(0, Math.min(0xffff, decoderQueueDepth));
    state.lastPresented = state.lastReceived;
    state.view.acknowledge(this.surfaceFeedback(state));
  }

  private surfaceFeedback(state: NativeSurfaceViewState) {
    return {
      presentedSequence: state.lastPresented,
      decoderQueueDepth: state.decoderQueueDepth,
      availableSlots: Math.max(
        0,
        state.view.result.maxInflightFrames - state.decoderQueueDepth,
      ),
    };
  }

  private async closeSurfaceView(surfaceId: SurfaceId): Promise<void> {
    const pending = this.pendingSurfaceViews.get(surfaceId);
    if (pending) pending.cancelled = true;
    const state = this.surfaceViews.get(surfaceId);
    if (!state) return;
    this.surfaceViews.delete(surfaceId);
    state.removeFrames();
    await state.view.close().catch(() => undefined);
  }

  private publicSession(record: YasTerminalRecord): YasSession {
    return {
      id: this.sessionId(record.handle),
      connectionId: this.id,
      ptyId: record.handle,
      tag: record.resourceTag ?? "",
      title: record.title ?? null,
      usedRows: record.usedRows,
      command: record.commandDisplay ?? null,
      state:
        record.lifecycle === YAS_TERMINAL_LIFECYCLE_EXITED
          ? "exited"
          : "active",
      exitStatus: exitStatus(record),
    };
  }

  private subscribeTerminalView(handle: bigint): void {
    this.desiredTerminalViews.add(handle);
    this.scheduleTerminalViewAdmissions();
  }

  private unsubscribeTerminalView(handle: bigint): void {
    this.desiredTerminalViews.delete(handle);
    if (this.terminalViewAdmissionBlocker === handle) {
      this.terminalViewAdmissionBlocker = null;
      this.scheduleTerminalViewAdmissions();
    }
    void this.closeView(handle);
  }

  /**
   * OPEN_VIEW results disclose the exact receive reservation only after the
   * server has created the view. Admit requests serially so one exhausted
   * aggregate budget produces bounded preview placeholders instead of a burst
   * of rejected requests. The desired handles remain queued and a later view
   * close retries them after releasing its receive lease.
   */
  private scheduleTerminalViewAdmissions(): void {
    if (
      this.terminalViewAdmissionScheduled ||
      this.terminalViewAdmissionBlocker !== null ||
      this.familyInitializationPending ||
      this.familyInitializationError != null ||
      this.disposed ||
      !this.session.ready
    )
      return;
    if (this.terminalViewAdmissionRunning) {
      this.terminalViewAdmissionWakePending = true;
      return;
    }
    this.terminalViewAdmissionScheduled = true;
    queueMicrotask(() => {
      this.terminalViewAdmissionScheduled = false;
      if (
        this.terminalViewAdmissionRunning ||
        this.terminalViewAdmissionBlocker !== null ||
        this.familyInitializationPending ||
        this.familyInitializationError != null ||
        this.disposed ||
        !this.session.ready
      )
        return;
      void this.drainTerminalViewAdmissions().catch((error) =>
        console.error("YAS terminal view admission queue failed", error),
      );
    });
  }

  private async drainTerminalViewAdmissions(): Promise<void> {
    if (this.terminalViewAdmissionRunning) return;
    this.terminalViewAdmissionRunning = true;
    try {
      while (
        !this.disposed &&
        this.session.ready &&
        !this.familyInitializationPending &&
        this.familyInitializationError == null &&
        this.terminalViewAdmissionBlocker === null
      ) {
        const handle = this.nextTerminalViewAdmissionCandidate();
        if (handle === undefined) return;
        const admissionEpoch = this.terminalViewAdmissionEpoch;
        const familyEpoch = this.familyInitializationEpoch;
        try {
          await this.openView(handle);
        } catch (error) {
          if (
            this.disposed ||
            !this.session.ready ||
            this.familyInitializationPending ||
            this.familyInitializationError != null
          )
            return;
          if (familyEpoch !== this.familyInitializationEpoch) continue;
          if (
            error instanceof YasResultError &&
            error.status === YAS_STATUS_RESOURCE_EXHAUSTED
          ) {
            if (admissionEpoch !== this.terminalViewAdmissionEpoch) continue;
            if (this.desiredTerminalViews.has(handle)) {
              this.terminalViewAdmissionBlocker = handle;
              this.reconcileTerminalViewAdmissionPriority();
              return;
            }
            continue;
          }
          // The delegate API is intentionally fire-and-forget. Retire an
          // unexpected failed candidate instead of leaking a rejected Promise
          // or retrying it in a tight loop; a later unsubscribe/subscribe cycle
          // can request it again.
          this.desiredTerminalViews.delete(handle);
          console.error("YAS terminal view admission failed", error);
        }
      }
    } finally {
      this.terminalViewAdmissionRunning = false;
      if (this.terminalViewAdmissionWakePending) {
        this.terminalViewAdmissionWakePending = false;
        this.scheduleTerminalViewAdmissions();
      }
    }
  }

  private resumeTerminalViewAdmissions(): void {
    this.terminalViewAdmissionBlocker = null;
    this.scheduleTerminalViewAdmissions();
  }

  /**
   * Receive budget was released somewhere in this session.
   *
   * Whatever gave it back, a view that could not be opened for want of budget
   * can be opened now — and not only the one recorded as the blocker: budget
   * is a per-session aggregate, so a lease released by Surface or Transfer is
   * as much of an invitation as one released by Terminal. Retrying only a
   * recorded blocker is why callers had to re-prime admissions on every event
   * to keep terminals repainting under pressure.
   */
  private onReceiveBudgetCapacity(): void {
    this.terminalViewAdmissionEpoch++;
    this.resumeTerminalViewAdmissions();
  }

  private nextTerminalViewAdmissionCandidate(): bigint | undefined {
    return [...this.desiredTerminalViews].find(
      (candidate) =>
        this.records.has(candidate) &&
        !this.views.has(candidate) &&
        !this.pendingViews.has(candidate),
    );
  }

  private reconcileTerminalViewAdmissionPriority(): void {
    if (this.terminalViewAdmissionBlocker === null) return;
    const candidate = this.nextTerminalViewAdmissionCandidate();
    if (candidate === undefined) {
      this.terminalViewAdmissionBlocker = null;
      return;
    }
    this.terminalViewAdmissionBlocker = candidate;
    this.evictLowerPriorityTerminalView();
  }

  /** Prefer a newly promoted main view over already-open preview views. */
  private evictLowerPriorityTerminalView(): void {
    if (
      this.disposed ||
      !this.session.ready ||
      this.familyInitializationPending ||
      this.familyInitializationError != null
    )
      return;
    const blocker = this.terminalViewAdmissionBlocker;
    if (blocker === null) return;
    let belowBlocker = false;
    for (const handle of this.desiredTerminalViews) {
      if (handle === blocker) {
        belowBlocker = true;
        continue;
      }
      if (!belowBlocker || !this.views.has(handle)) continue;
      void this.closeView(handle).catch((error) =>
        console.error("YAS terminal preview eviction failed", error),
      );
      return;
    }
  }

  private async openView(handle: bigint): Promise<void> {
    const pending = this.pendingViews.get(handle);
    if (pending) {
      if (!pending.cancelled) return pending.promise;
      await pending.promise.catch(() => undefined);
      return this.openView(handle);
    }
    const entry: PendingTerminalView = {
      promise: Promise.resolve(),
      cancelled: false,
    };
    entry.promise = this.openTerminalView(handle, entry);
    this.pendingViews.set(handle, entry);
    void entry.promise.then(
      () => {
        if (this.pendingViews.get(handle) === entry)
          this.pendingViews.delete(handle);
      },
      () => {
        if (this.pendingViews.get(handle) === entry)
          this.pendingViews.delete(handle);
      },
    );
    return entry.promise;
  }

  private async openTerminalView(
    handle: bigint,
    pending: PendingTerminalView,
  ): Promise<void> {
    if (
      this.views.has(handle) ||
      !this.records.has(handle) ||
      !this.session.ready
    )
      return;
    const record = this.records.get(handle)!;
    const size = this.effectiveViewSize(handle) ?? {
      rows: record.rows,
      cols: record.cols,
    };
    const terminal = this.terminal;
    const view = await terminal.openView({
      terminalHandle: handle,
      rows: size.rows,
      cols: size.cols,
      maxFps: 60,
      codecVersions: [YAS_TERMINAL_GRID_CODEC_V1],
    });
    if (
      this.disposed ||
      pending.cancelled ||
      !this.session.ready ||
      this.terminalClient !== terminal ||
      !this.records.has(handle) ||
      this.views.has(handle)
    ) {
      await view.close();
      return;
    }
    const state: NativeTerminalViewState = {
      view,
      removeFrames: () => undefined,
      grids: new Map(),
      pendingSequences: [],
      lastPresented: (view.result.firstSequence - 1) >>> 0,
    };
    state.removeFrames = view.subscribe((frame) => {
      const baseSequence =
        frame.flags & YAS_TERMINAL_FRAME_KEYFRAME
          ? undefined
          : (frame.explicitBase ?? (frame.sequence - 1) >>> 0);
      const grid = this.terminalGrid(
        frame,
        baseSequence === undefined
          ? null
          : (state.grids.get(baseSequence) ?? null),
        view.result.maxDecodedFrame,
      );
      state.grids.set(frame.sequence, grid);
      state.pendingSequences.push(frame.sequence);
      this.store.handleUpdate(handle, encodeBrowserTerminalGrid(grid));
      this.pruneGrids(state);
    });
    this.views.set(handle, state);
    if (this.focusedSessionId === this.sessionId(handle))
      void this.terminal.setFocus(view.result.viewId, true);
  }

  private terminalGrid(
    frame: YasTerminalFrameEvent,
    base: YasTerminalGridState | null,
    maximum: number,
  ): YasTerminalGridState {
    // Kept as an indirection so renderer tests can stub frame decoding without
    // constructing a network session.
    return decodeTerminalGridV1(frame, base, maximum);
  }

  /**
   * Configure an open Terminal view, reopening it if the server refuses.
   *
   * A view's frame reservation is fixed when it opens and the CONFIGURE_VIEW
   * Result carries no way to revise it, so a geometry that would outgrow the
   * reservation comes back as RESOURCE_EXHAUSTED with the view untouched.
   * Closing it returns the handle to the admission queue, which reopens it at
   * the size {@link effectiveViewSize} now reports and gets a bound sized for
   * it. Ordinary resizes stay inside the bound and never reach this path.
   */
  private configureView(
    handle: bigint,
    configuration: YasTerminalViewConfiguration,
  ): void {
    const state = this.views.get(handle);
    if (!state) return;
    void state.view.configure(configuration).catch((error) => {
      if (
        !(error instanceof YasResultError) ||
        error.status !== YAS_STATUS_RESOURCE_EXHAUSTED
      ) {
        if (this.views.get(handle) === state)
          console.error("YAS terminal view configuration failed", error);
        return;
      }
      if (this.views.get(handle) !== state) return;
      void this.closeView(handle).catch((closeError) =>
        console.error("YAS terminal view reopen failed", closeError),
      );
    });
  }

  private async closeView(handle: bigint): Promise<void> {
    const pending = this.pendingViews.get(handle);
    if (pending) {
      pending.cancelled = true;
      void pending.promise.then(
        () => this.resumeTerminalViewAdmissions(),
        () => this.resumeTerminalViewAdmissions(),
      );
    }
    const state = this.views.get(handle);
    if (!state) return;
    this.views.delete(handle);
    state.removeFrames();
    await state.view.close().catch(() => undefined);
    this.resumeTerminalViewAdmissions();
  }

  private closeViewsLocal(): void {
    for (const pending of this.pendingViews.values()) pending.cancelled = true;
    for (const state of this.views.values()) {
      state.removeFrames();
      state.view.closeLocal();
    }
    this.views.clear();
    for (const state of this.surfaceViews.values()) {
      state.removeFrames();
      state.view.closeLocal();
    }
    this.surfaceViews.clear();
    this.terminalViewAdmissionBlocker = null;
  }

  private acknowledge(handle: bigint): void {
    const state = this.views.get(handle);
    const sequence = state?.pendingSequences.shift();
    if (!state || sequence === undefined) return;
    state.lastPresented = sequence;
    state.view.acknowledge(this.feedback(state));
    this.pruneGrids(state);
  }

  private feedback(state: NativeTerminalViewState) {
    return state.view.feedback(
      state.lastPresented,
      Math.min(state.pendingSequences.length, 0xff),
      Math.max(
        0,
        state.view.result.maxInflightFrames - state.pendingSequences.length,
      ),
    );
  }

  private pruneGrids(state: NativeTerminalViewState): void {
    const keep = state.view.result.maxInflightFrames + 1;
    while (state.grids.size > keep) {
      const first = state.grids.keys().next().value as number | undefined;
      if (first === undefined || first === state.lastPresented) break;
      state.grids.delete(first);
    }
  }

  private configureDisplayRate(fps: number): void {
    const displayFps = Math.max(1, Math.min(0xffff, fps));
    if (displayFps !== this.displayFps) {
      this.displayFps = displayFps;
      for (const surfaceId of this.surfaceMounts.keys())
        void this.refreshNativeSurfaceView(surfaceId);
    }
    for (const handle of [...this.views.keys()])
      this.configureView(handle, {
        maxFps: displayFps,
      });
  }

  private applyEffectiveViewSize(handle: bigint): void {
    const size = this.effectiveViewSize(handle);
    const state = this.views.get(handle);
    if (size && state)
      this.configureView(handle, { rows: size.rows, cols: size.cols });
  }

  private effectiveViewSize(handle: bigint): ViewSize | null {
    const sizes = [...(this.viewSizes.get(handle)?.values() ?? [])];
    if (sizes.length === 0) return null;
    return (
      sizes.find((size) => size.isActive?.()) ??
      sizes.reduce((best, size) =>
        size.rows * size.cols > best.rows * best.cols ? size : best,
      )
    );
  }

  private async browserDragContent(
    request: YasSelectionGet,
  ): Promise<{ bytes: Uint8Array; contentHash: Uint8Array }> {
    if (request.target.kind !== "drag")
      throw new Error("native browser drag handler received a slot GET");
    const drag = this.browserDrag;
    if (!drag || drag.cancelled)
      throw new Error("native browser drag is no longer available");
    const identity = await drag.identity;
    if (
      request.target.dragHandle !== identity.dragHandle ||
      request.target.revision !== identity.revision
    )
      throw new Error("Selection GET names another browser drag");
    const pending = drag.items[request.target.itemIndex];
    if (!pending) throw new Error("Selection GET item index is out of range");
    const item = await pending.ready;
    if (!item) throw new Error("browser drag was cancelled");
    if (
      request.mime !== item.mime &&
      !(
        request.mime.startsWith("text/plain") &&
        item.mime.startsWith("text/plain")
      ) &&
      request.mime !== "application/octet-stream"
    )
      throw new Error(`browser drag cannot provide ${request.mime}`);
    const bytes =
      item.data instanceof Uint8Array
        ? new Uint8Array(item.data)
        : new Uint8Array(await item.data.arrayBuffer());
    const { blake3_hash } = await import("@yas-run/browser");
    return { bytes, contentHash: blake3_hash(bytes) };
  }

  private cancelBrowserDrag(reason: string): void {
    const drag = this.browserDrag;
    if (!drag || drag.cancelled) return;
    drag.cancelled = true;
    this.browserDrag = null;
    for (const item of drag.items) item.resolve(null);
    void drag.identity.then(({ dragHandle, revision }) =>
      this.selection
        .dragCancel(dragHandle, revision, operationId(), reason)
        .catch(() => undefined),
    );
  }

  private async setSelection(
    slot: number,
    mime: string,
    data: Uint8Array,
  ): Promise<void> {
    if (data.length <= YAS_SELECTION_MAX_INLINE_BYTES) {
      await this.selection.set(slot, operationId(), [{ mime, data }]);
      return;
    }
    const { blake3_hash } = await import("@yas-run/browser");
    const hash = blake3_hash(data);
    const batch = await this.selection.beginSet(slot, operationId(), [
      {
        mime,
        byteLength: BigInt(data.length),
        contentHash: hash,
        initialReceiveCredit: BigInt(data.length),
      },
    ]);
    const transfer = batch.transfers[0];
    if (!transfer) throw new Error("Selection SET_BEGIN omitted its Transfer");
    await transfer.write(data);
    transfer.closeWrite();
    await transfer.closed;
    await this.selection.commitSet(batch.stagingHandle, operationId());
  }

  private sessionId(handle: bigint): SessionId {
    return `${this.id}:terminal:${handle.toString(16)}`;
  }

  private handleForSession(sessionId: SessionId): bigint {
    const session = this.sessions.get(sessionId);
    if (!session || typeof session.ptyId !== "bigint")
      throw new Error(`Unknown native session ${sessionId}`);
    return session.ptyId;
  }

  private requireSession(sessionId: SessionId): YasSession {
    const session = this.sessions.get(sessionId);
    if (!session) throw new Error(`Unknown session ${sessionId}`);
    return session;
  }

  private visibleHandles(): bigint[] {
    return [...this.views.keys()];
  }

  private waitForRecord(handle: bigint): Promise<void> {
    if (this.records.has(handle)) return Promise.resolve();
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        remove();
        reject(new Error("Terminal state did not publish created handle"));
      }, 10_000);
      const remove = this.subscribe(() => {
        if (!this.records.has(handle)) return;
        clearTimeout(timer);
        remove();
        resolve();
      });
    });
  }

  private emitScrollAnchor(sessionId: SessionId, offset: number): void {
    const handle = this.handleForSession(sessionId);
    for (const listener of this.scrollAnchorListeners.get(handle) ?? [])
      listener(offset);
  }

  private refreshSnapshot(): void {
    const status = this.connectionStatus;
    const supportsTerminal = this.supportsStateCatalogue(
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_WATCH,
      YAS_TERMINAL_UNWATCH,
      YAS_TERMINAL_STATE,
      YAS_TERMINAL_STATE_ACK,
    );
    const supportsSurface = this.supportsStateCatalogue(
      YAS_FAMILY_SURFACE,
      YAS_SURFACE_WATCH,
      YAS_SURFACE_UNWATCH,
      YAS_SURFACE_STATE,
      YAS_SURFACE_STATE_ACK,
    );
    this.snapshot = {
      ...this.snapshot,
      status,
      ready:
        this.session.ready &&
        !this.familyInitializationPending &&
        this.familyInitializationError === null,
      generation: this.generation,
      error: this.familyInitializationError ?? this.transport.lastError,
      sessions: [...this.sessions.values()],
      focusedSessionId: this.focusedSessionId,
      supportsCopyRange:
        supportsTerminal &&
        this.session.operationAdvertised(
          YAS_FAMILY_TERMINAL,
          YAS_CLASS_REQUEST,
          YAS_TERMINAL_COPY_RANGE,
        ),
      supportsCompositor: supportsSurface,
      supportsSurfaceTouch: this.supportsSurfaceTouch,
      supportsSurfaceTextInput: this.supportsSurfaceTextInput,
      supportsAudio: this.desktopMedia?.supportsAudio ?? false,
      supportsClientControl: this.desktopMedia?.supportsClientControl ?? false,
      supportsDesktop: this.desktopMedia?.supportsDesktop ?? false,
      supportsDesktopMedia: this.desktopMedia?.supportsDesktopMedia ?? false,
      supportsKv: this.native.supports("kv"),
      supportsFsSync: this.native.supports("fs"),
      supportsGit: this.native.supports("git"),
      supportsLsp: this.native.supports("lsp"),
      supportsChannels: this.native.supports("channel"),
      supportsChannelWatch: this.native.supports("channel"),
      supportsExtensions: this.native.supports("extension"),
      bootGeneration: this.session.hello
        ? bootIdAsBigInt(this.session.hello.bootId)
        : null,
    };
    this.emit();
    if (this.isReady()) {
      for (const listener of this.readyListeners) listener();
      this.readyListeners.clear();
    }
  }

  private emptySnapshot(): YasConnectionSnapshot {
    return {
      id: this.id,
      status: this.transport.status,
      ready: false,
      supportsRestart: true,
      supportsCopyRange: false,
      supportsCompositor: false,
      supportsSurfaceTouch: false,
      supportsSurfaceTextInput: false,
      supportsAudio: false,
      supportsClientControl: false,
      supportsFsSync: false,
      supportsGit: false,
      supportsLsp: false,
      supportsKv: false,
      supportsDesktop: false,
      supportsChannels: false,
      supportsChannelWatch: false,
      supportsExtensions: false,
      supportsDesktopMedia: false,
      retryCount: 0,
      bootGeneration: null,
      generation: 0,
      error: null,
      sessions: [],
      focusedSessionId: null,
    };
  }

  private emit(): void {
    for (const listener of this.listeners) listener();
  }
}

function bootIdAsBigInt(bootId: Uint8Array): bigint {
  let value = 0n;
  for (const byte of bootId) value = (value << 8n) | BigInt(byte);
  return value;
}

function exitStatus(record: YasTerminalRecord): number | null {
  if (record.lifecycle !== YAS_TERMINAL_LIFECYCLE_EXITED) return null;
  if (record.exit?.kind === YAS_TERMINAL_EXIT_KIND_CODE)
    return record.exit.code;
  if (
    record.exit?.kind === YAS_TERMINAL_EXIT_KIND_SIGNAL &&
    record.exit.nativeSignal > 0
  )
    return -record.exit.nativeSignal;
  return EXIT_STATUS_UNKNOWN;
}

function operationId(): Uint8Array {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  return bytes;
}

function monotonicNs(): bigint {
  return (
    BigInt(Math.floor((globalThis.performance?.now() ?? Date.now()) * 1_000)) *
    1_000n
  );
}

function xtermButton(value: number): number {
  switch (value & 3) {
    case 0:
      return YAS_TERMINAL_MOUSE_BUTTON_LEFT;
    case 1:
      return YAS_TERMINAL_MOUSE_BUTTON_MIDDLE;
    case 2:
      return YAS_TERMINAL_MOUSE_BUTTON_RIGHT;
    default:
      return YAS_TERMINAL_MOUSE_BUTTON_NONE;
  }
}

function compareBytes(left: Uint8Array, right: Uint8Array): number {
  for (let index = 0; index < Math.min(left.length, right.length); index++) {
    const order = left[index]! - right[index]!;
    if (order !== 0) return order;
  }
  return left.length - right.length;
}

function sameHandles(
  left: ReadonlySet<bigint>,
  right: ReadonlySet<bigint>,
): boolean {
  if (left.size !== right.size) return false;
  for (const handle of left) if (!right.has(handle)) return false;
  return true;
}

function sameBytes(left: Uint8Array, right: Uint8Array): boolean {
  return (
    left.length === right.length && left.every((value, i) => value === right[i])
  );
}

function orderedMimes(values: readonly string[]): string[] {
  return [
    ...new Set(
      values.filter((value) => value.length > 0 && !value.includes("\0")),
    ),
  ].sort();
}

function deferredDragItem(): NativeDragItemData {
  let settle!: (value: NativeDragPayload | null) => void;
  let settled = false;
  const ready = new Promise<NativeDragPayload | null>((resolve) => {
    settle = resolve;
  });
  return {
    ready,
    resolve(value) {
      if (settled) return;
      settled = true;
      settle(value);
    },
  };
}

function fixed32(value: number): bigint {
  if (!Number.isFinite(value)) return 0n;
  return BigInt(Math.round(value * 0x1_0000_0000));
}

function fixed32Integer(value: bigint): number {
  const rounded = (value + (1n << 31n)) >> 32n;
  return Number(rounded);
}

function effectiveSurfaceSize(
  views: Iterable<{ width: number; height: number; scale120: number }>,
): {
  logicalWidth: number;
  logicalHeight: number;
  physicalWidth: number;
  physicalHeight: number;
  scale120: number;
} | null {
  const effectiveScale = (scale: number) => (scale >= 120 ? scale : 120);
  const logical = (pixels: number, scale: number) =>
    Math.floor(
      (pixels * 120 + effectiveScale(scale) / 2) / effectiveScale(scale),
    );
  let minimumWidth: {
    logical: number;
    pixels: number;
    scale120: number;
  } | null = null;
  let minimumHeight: {
    logical: number;
    pixels: number;
    scale120: number;
  } | null = null;
  let scale120 = 0;
  for (const view of views) {
    if (view.width <= 0 || view.height <= 0) continue;
    const logicalWidth = logical(view.width, view.scale120);
    const logicalHeight = logical(view.height, view.scale120);
    if (!minimumWidth || logicalWidth < minimumWidth.logical)
      minimumWidth = {
        logical: logicalWidth,
        pixels: view.width,
        scale120: view.scale120,
      };
    if (!minimumHeight || logicalHeight < minimumHeight.logical)
      minimumHeight = {
        logical: logicalHeight,
        pixels: view.height,
        scale120: view.scale120,
      };
    scale120 = Math.max(scale120, view.scale120);
  }
  if (!minimumWidth || !minimumHeight) return null;
  const physical = (
    minimum: { logical: number; pixels: number; scale120: number },
    scale: number,
  ) =>
    effectiveScale(minimum.scale120) === scale
      ? minimum.pixels
      : Math.max(1, Math.floor((Math.max(1, minimum.logical) * scale) / 120));
  const chosenScale = effectiveScale(scale120);
  return {
    logicalWidth: minimumWidth.logical,
    logicalHeight: minimumHeight.logical,
    physicalWidth: physical(minimumWidth, chosenScale),
    physicalHeight: physical(minimumHeight, chosenScale),
    scale120: chosenScale,
  };
}

function effectiveSurfaceMount(
  mounts: Iterable<NativeSurfaceMount>,
  nativeWidth: number,
  nativeHeight: number,
  displayFps: number,
  maximumFps: number,
  maximumDimension: number,
  maximumPixels: bigint,
  negotiatedMaximumFps: number,
): { width: number; height: number; maxFps: number } {
  let unscaled = false;
  let width = 0;
  let height = 0;
  let requestedFps = 0;
  let uncappedFps = false;
  for (const mount of mounts) {
    if (mount.target === null) unscaled = true;
    else {
      width = Math.max(width, Math.round(mount.target.width));
      height = Math.max(height, Math.round(mount.target.height));
    }
    if (mount.maxFps <= 0) uncappedFps = true;
    else requestedFps = Math.max(requestedFps, Math.round(mount.maxFps));
  }
  if (unscaled || width <= 0 || height <= 0) {
    width = nativeWidth;
    height = nativeHeight;
  }
  const pixelLimit = Number(maximumPixels);
  const scale = Math.min(
    1,
    maximumDimension / width,
    maximumDimension / height,
    Math.sqrt(pixelLimit / (width * height)),
  );
  width = Math.max(1, Math.floor(width * scale));
  height = Math.max(1, Math.floor(height * scale));
  let maxFps = requestedFps;
  if (uncappedFps || maxFps <= 0)
    maxFps = Math.max(maxFps, Math.round(displayFps));
  if (maximumFps > 0) maxFps = Math.min(maxFps, maximumFps);
  maxFps = Math.max(1, Math.min(negotiatedMaximumFps, 0xffff, maxFps));
  return { width, height, maxFps };
}

function surfaceCodecName(codec: number): string {
  if (codec === YAS_SURFACE_CODEC_H264_V1) return "yas/h264-v1";
  if (codec === YAS_SURFACE_CODEC_AV1_V1) return "yas/av1-v1";
  return "yas/png-v1";
}

function nativeSurfaceCodecs(support: number): number[] {
  if (support === 0)
    return [
      YAS_SURFACE_CODEC_H264_V1,
      YAS_SURFACE_CODEC_AV1_V1,
      YAS_SURFACE_CODEC_PNG_V1,
    ];
  const codecs: number[] = [];
  if (support & (CODEC_SUPPORT_H264 | CODEC_SUPPORT_H264_444))
    codecs.push(YAS_SURFACE_CODEC_H264_V1);
  if (support & (CODEC_SUPPORT_AV1 | CODEC_SUPPORT_AV1_444))
    codecs.push(YAS_SURFACE_CODEC_AV1_V1);
  codecs.push(YAS_SURFACE_CODEC_PNG_V1);
  return codecs;
}

const evdevHid = new Map<number, number>([
  [1, yasGenerated.YAS_SURFACE_KEY_ESCAPE],
  [2, yasGenerated.YAS_SURFACE_KEY_1],
  [3, yasGenerated.YAS_SURFACE_KEY_2],
  [4, yasGenerated.YAS_SURFACE_KEY_3],
  [5, yasGenerated.YAS_SURFACE_KEY_4],
  [6, yasGenerated.YAS_SURFACE_KEY_5],
  [7, yasGenerated.YAS_SURFACE_KEY_6],
  [8, yasGenerated.YAS_SURFACE_KEY_7],
  [9, yasGenerated.YAS_SURFACE_KEY_8],
  [10, yasGenerated.YAS_SURFACE_KEY_9],
  [11, yasGenerated.YAS_SURFACE_KEY_0],
  [12, yasGenerated.YAS_SURFACE_KEY_MINUS],
  [13, yasGenerated.YAS_SURFACE_KEY_EQUAL],
  [14, yasGenerated.YAS_SURFACE_KEY_BACKSPACE],
  [15, yasGenerated.YAS_SURFACE_KEY_TAB],
  [16, yasGenerated.YAS_SURFACE_KEY_Q],
  [17, yasGenerated.YAS_SURFACE_KEY_W],
  [18, yasGenerated.YAS_SURFACE_KEY_E],
  [19, yasGenerated.YAS_SURFACE_KEY_R],
  [20, yasGenerated.YAS_SURFACE_KEY_T],
  [21, yasGenerated.YAS_SURFACE_KEY_Y],
  [22, yasGenerated.YAS_SURFACE_KEY_U],
  [23, yasGenerated.YAS_SURFACE_KEY_I],
  [24, yasGenerated.YAS_SURFACE_KEY_O],
  [25, yasGenerated.YAS_SURFACE_KEY_P],
  [26, yasGenerated.YAS_SURFACE_KEY_BRACKET_LEFT],
  [27, yasGenerated.YAS_SURFACE_KEY_BRACKET_RIGHT],
  [28, yasGenerated.YAS_SURFACE_KEY_ENTER],
  [29, yasGenerated.YAS_SURFACE_KEY_CONTROL_LEFT],
  [30, yasGenerated.YAS_SURFACE_KEY_A],
  [31, yasGenerated.YAS_SURFACE_KEY_S],
  [32, yasGenerated.YAS_SURFACE_KEY_D],
  [33, yasGenerated.YAS_SURFACE_KEY_F],
  [34, yasGenerated.YAS_SURFACE_KEY_G],
  [35, yasGenerated.YAS_SURFACE_KEY_H],
  [36, yasGenerated.YAS_SURFACE_KEY_J],
  [37, yasGenerated.YAS_SURFACE_KEY_K],
  [38, yasGenerated.YAS_SURFACE_KEY_L],
  [39, yasGenerated.YAS_SURFACE_KEY_SEMICOLON],
  [40, yasGenerated.YAS_SURFACE_KEY_QUOTE],
  [41, yasGenerated.YAS_SURFACE_KEY_BACKQUOTE],
  [42, yasGenerated.YAS_SURFACE_KEY_SHIFT_LEFT],
  [43, yasGenerated.YAS_SURFACE_KEY_BACKSLASH],
  [44, yasGenerated.YAS_SURFACE_KEY_Z],
  [45, yasGenerated.YAS_SURFACE_KEY_X],
  [46, yasGenerated.YAS_SURFACE_KEY_C],
  [47, yasGenerated.YAS_SURFACE_KEY_V],
  [48, yasGenerated.YAS_SURFACE_KEY_B],
  [49, yasGenerated.YAS_SURFACE_KEY_N],
  [50, yasGenerated.YAS_SURFACE_KEY_M],
  [51, yasGenerated.YAS_SURFACE_KEY_COMMA],
  [52, yasGenerated.YAS_SURFACE_KEY_PERIOD],
  [53, yasGenerated.YAS_SURFACE_KEY_SLASH],
  [54, yasGenerated.YAS_SURFACE_KEY_SHIFT_RIGHT],
  [56, yasGenerated.YAS_SURFACE_KEY_ALT_LEFT],
  [57, yasGenerated.YAS_SURFACE_KEY_SPACE],
  [58, yasGenerated.YAS_SURFACE_KEY_CAPS_LOCK],
  [59, yasGenerated.YAS_SURFACE_KEY_F1],
  [60, yasGenerated.YAS_SURFACE_KEY_F2],
  [61, yasGenerated.YAS_SURFACE_KEY_F3],
  [62, yasGenerated.YAS_SURFACE_KEY_F4],
  [63, yasGenerated.YAS_SURFACE_KEY_F5],
  [64, yasGenerated.YAS_SURFACE_KEY_F6],
  [65, yasGenerated.YAS_SURFACE_KEY_F7],
  [66, yasGenerated.YAS_SURFACE_KEY_F8],
  [67, yasGenerated.YAS_SURFACE_KEY_F9],
  [68, yasGenerated.YAS_SURFACE_KEY_F10],
  [87, yasGenerated.YAS_SURFACE_KEY_F11],
  [88, yasGenerated.YAS_SURFACE_KEY_F12],
  [97, yasGenerated.YAS_SURFACE_KEY_CONTROL_RIGHT],
  [100, yasGenerated.YAS_SURFACE_KEY_ALT_RIGHT],
  [102, yasGenerated.YAS_SURFACE_KEY_HOME],
  [103, yasGenerated.YAS_SURFACE_KEY_ARROW_UP],
  [104, yasGenerated.YAS_SURFACE_KEY_PAGE_UP],
  [105, yasGenerated.YAS_SURFACE_KEY_ARROW_LEFT],
  [106, yasGenerated.YAS_SURFACE_KEY_ARROW_RIGHT],
  [107, yasGenerated.YAS_SURFACE_KEY_END],
  [108, yasGenerated.YAS_SURFACE_KEY_ARROW_DOWN],
  [109, yasGenerated.YAS_SURFACE_KEY_PAGE_DOWN],
  [110, yasGenerated.YAS_SURFACE_KEY_INSERT],
  [111, yasGenerated.YAS_SURFACE_KEY_DELETE],
  [125, yasGenerated.YAS_SURFACE_KEY_SUPER_LEFT],
  [126, yasGenerated.YAS_SURFACE_KEY_SUPER_RIGHT],
]);

function evdevToHid(evdev: number): number | undefined {
  return evdevHid.get(evdev);
}

function surfaceModifiers(keys: ReadonlySet<number>): number {
  return (
    (keys.has(42) || keys.has(54) ? YAS_SURFACE_MODIFIER_SHIFT : 0) |
    (keys.has(29) || keys.has(97) ? YAS_SURFACE_MODIFIER_CONTROL : 0) |
    (keys.has(56) || keys.has(100) ? YAS_SURFACE_MODIFIER_ALT : 0) |
    (keys.has(125) || keys.has(126) ? YAS_SURFACE_MODIFIER_SUPER : 0)
  );
}
