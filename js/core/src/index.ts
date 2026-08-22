export { YasWorkspace, consoleLogger, nullLogger } from "./YasWorkspace";
export type { YasLogger, YasWorkspaceConnection } from "./YasWorkspace";
export { YasNativeWorkspaceConnection } from "./YasNativeWorkspaceConnection";
export { YasNativeRelayTransport } from "./yas/nativeRelayTransport";
export { YasActivityStore } from "./activity";
export type {
  YasActivity,
  YasActivityHandle,
  YasActivityUpdate,
} from "./activity";

export {
  YAS_TERMINAL_CATALOG_SEARCH_SOURCE_TITLE as SEARCH_SOURCE_TITLE,
  YAS_TERMINAL_CATALOG_SEARCH_SOURCE_VISIBLE as SEARCH_SOURCE_VISIBLE,
  YAS_TERMINAL_CATALOG_SEARCH_SOURCE_SCROLLBACK as SEARCH_SOURCE_SCROLLBACK,
} from "./yas/generated";
export type {
  AwaitSessionExitOptions,
  CreateSessionOptions,
  SurfaceTarget,
} from "./workspaceConnectionTypes";

export type { YasWasmModule } from "./TerminalStore";
export { AudioPlayer } from "./AudioPlayer";
export {
  releaseRecordingAudioSession,
  retainRecordingAudioSession,
} from "./audioSession";
export { NumberRing, SurfaceFrameHistory, SurfaceStore } from "./SurfaceStore";
export type {
  SurfaceFrameCallback,
  SurfaceEventCallback,
  SurfaceFrameSample,
  ServerClockSample,
  RemoteSurfaceInput,
  RemoteSurfacePointer,
  SurfaceCursorImage,
  SurfaceCursorRect,
  SurfaceTextInputEvent,
  SurfaceTextInputState,
} from "./SurfaceStore";
export {
  estimateSourceToReceiveMs,
  sourceTimestampDelta,
  wrappingTimestampDelta,
} from "./SurfaceStore";

export { clampZoom, driveSurfaceResize } from "./surfaceResize";
export type { SurfaceResizeTarget, SurfaceZoom } from "./surfaceResize";

export { measureCell, cssFontFamily } from "./measure";
export type { CellMetrics } from "./measure";

export { assessUrl, escapeUrlForDisplay, openUrlSafely } from "./urlSecurity";
export type { UrlAssessment, UrlVerdict, UrlReason } from "./urlSecurity";

export { createShareTransport } from "./transports/webrtc-share";

/** YAS v1 session, Transfer, Relay, Font, and browser-edge clients. */
export * from "./yas";
export * from "./workspaceSessionKv";

/** HTTP/1.1 over a relayed stream, for the preview service worker. */
export * from "./http1";
/** Preview targets and the /x/ bootstrap prefix. */
export * from "./preview";
/** Durable backend workspace sessions. */
export * from "./workspaceSessions";
/** Durable per-device workspace-session attachment ordering. */
export * from "./workspaceSessionDevices";

// Product-model types and presentation helpers retained by the UI.
export {
  MENU_NODE_CHECKMARK,
  MENU_NODE_ENABLED,
  MENU_NODE_RADIO,
  MENU_NODE_SEPARATOR,
  MENU_NODE_SUBMENU,
  MENU_NODE_VISIBLE,
  TRAY_HAS_MENU,
  TRAY_ITEM_IS_MENU,
  TRAY_MENU_OK,
  TRAY_STATUS_NEEDS_ATTENTION,
  TRAY_STATUS_PASSIVE,
} from "./desktopModel";
export type {
  DesktopId,
  DesktopImage,
  DesktopNotification,
  DesktopRevision,
  TrayItem,
  TrayMenu,
  TrayMenuNode,
} from "./desktopModel";
export {
  ACTIVE_CAMERA,
  ACTIVE_MICROPHONE,
  AUDIO_CODEC_OPUS,
  AUDIO_CODEC_PCM,
  MPRIS_CAN_CONTROL,
  MPRIS_CAN_GO_NEXT,
  MPRIS_CAN_GO_PREVIOUS,
  MPRIS_CAN_PAUSE,
  MPRIS_CAN_PLAY,
  MPRIS_CAN_RAISE,
  MPRIS_CAN_SEEK,
  RUNTIME_CAMERA,
  RUNTIME_MICROPHONE,
  VIDEO_CODEC_AV1,
  VIDEO_CODEC_AV1_444,
  VIDEO_CODEC_H264,
  VIDEO_CODEC_H264_444,
  VIDEO_CODEC_MJPEG,
  cameraCodecLabel,
  cameraCodecProbeOutcomes,
  cameraCodecProbeReport,
  probeCameraCodecs,
  probeOpusMicrophone,
} from "./mediaModel";
export type {
  CameraCodecProbeOutcome,
  CameraQuality,
  MediaId,
  MprisAction,
  MprisArtwork,
  MprisPlayer,
  PortalChoiceValue,
  PortalRequest,
  ScreenCastState,
} from "./mediaModel";
export {
  FS_ENTRY_DIR,
  FS_ENTRY_FILE,
  FS_ENTRY_LINK_DIR,
  FS_ENTRY_NO_CONTENT,
  FS_ENTRY_SYMLINK,
  FS_ENTRY_TYPE_MASK,
  FS_ENTRY_UNREADABLE,
  FS_ENTRY_UNSTABLE,
} from "./fsModel";
export type {
  FsFileIndex,
  FsGrepFile,
  FsGrepOptions,
  FsGrepResult,
} from "./fsModel";
export {
  GIT_CLOSED_CLIENT_REQUEST,
  GIT_CLOSED_CONNECTION_LOST,
  GIT_CLOSED_PERMISSION_LOST,
  GIT_CLOSED_REPO_GONE,
  GIT_CLOSED_RESOURCE_LIMIT,
  GIT_COMMITS_MORE,
  GIT_DIFF_UNTRACKED,
  GIT_ENDPOINT_COMMIT,
  GIT_ENDPOINT_EMPTY,
  GIT_ENDPOINT_INDEX,
  GIT_ENDPOINT_WORKTREE,
  GIT_HEAD_DETACHED,
  GIT_HEAD_UNBORN,
  GIT_LOG_FULL_MESSAGE,
  GIT_LOG_TOPO,
  GIT_OID_NONE,
  GIT_OP_BISECT,
  GIT_OP_CHERRY_PICK,
  GIT_OP_MERGE,
  GIT_OP_REBASE,
  GIT_OP_REVERT,
  GIT_REF_PEELED_VALID,
  GIT_REF_SYMBOLIC,
  GIT_STATUS_ENTRY_CONFLICTED,
  GIT_STATUS_OK,
  GIT_UPSTREAM_COUNTS_VALID,
  GIT_UPSTREAM_GONE,
  GIT_WORKTREE_BARE,
  GIT_WORKTREE_CURRENT,
  GIT_WORKTREE_DETACHED,
  GIT_WORKTREE_LOCKED,
  GIT_WORKTREE_MAIN,
  GIT_WORKTREE_PRUNABLE,
  gitOidFromHex,
  gitOidHex,
} from "./gitModel";
export type { GitOid, GitPatchRecord, GitWorktreeRecord } from "./gitModel";
export { GitStateMirror, GitStatusError } from "./gitModel";
export {
  LSP_COMPLETION_DEPRECATED,
  LSP_COMPLETION_PRESELECT,
  LSP_COMPLETION_SNIPPET,
  LSP_MARKUP_MARKDOWN,
  LSP_PHASE_FAILED,
  LSP_PHASE_INDEXING,
  LSP_PHASE_INITIALIZING,
  LSP_PHASE_READY,
  LSP_PHASE_SPAWNING,
  LSP_SEVERITY_ERROR,
  LSP_SEVERITY_INFO,
  LSP_SEVERITY_WARNING,
  LSP_STATUS_OK,
  LSP_STATUS_WARMING,
  lspStatusText,
} from "./lspModel";
export type {
  YasNativeChannelHandle as ChannelHandle,
  YasNativeChannelNamesWatch as ChannelNamesWatch,
  YasNativeChannelOpenOptions as ChannelOpenOptions,
} from "./yas/nativeChannelFacade";
export type { NetOpenOptions, NetStream } from "./netModel";
export { formatExtensionId, parseModuleDigest } from "./extensionModel";

export { DEFAULT_FONT, DEFAULT_FONT_SIZE, DEFAULT_TEXT_GAMMA } from "./types";
export {
  CODEC_SUPPORT_H264,
  CODEC_SUPPORT_AV1,
  CODEC_SUPPORT_H264_444,
  CODEC_SUPPORT_AV1_444,
} from "./surfaceModel";
export type { YasTransportMessage } from "./types";

export {
  EXIT_STATUS_UNKNOWN,
  exitCodeFromStatus,
  formatExitStatus,
} from "./exit-status";

export { Notifier } from "./reactive";
export type { ReactiveStore } from "./reactive";

export type {
  YasConnectionSnapshot,
  YasClientOrigin,
  YasClientInfo,
  YasClientList,
  YasClientAuxSubscription,
  YasClientSurfaceSubscription,
  YasClientTerminalSubscription,
  YasDebug,
  YasSearchResult,
  YasSurface,
  YasSurfaceOrigin,
  YasWorkspaceSnapshot,
  YasTransport,
  YasSession,
  ConnectionId,
  ConnectionStatus,
  CopyRangeResult,
  SessionId,
  SurfaceId,
  TerminalId,
  TerminalPalette,
  TransportConfig,
} from "./types";

export {
  SURFACE_POINTER_DOWN,
  SURFACE_POINTER_UP,
  SURFACE_POINTER_MOVE,
  CLIENT_DISCONNECT_REASON_MAX_BYTES,
  clientDisconnectReasonByteLength,
} from "./input";

export { PALETTES } from "./palettes";

export { MOUSE_DOWN, MOUSE_UP, MOUSE_MOVE } from "./input";
export { keyToBytes, ctrlCharToByte, encoder } from "./keyboard";

export type { GlRenderer, RendererBackend } from "./gl-renderer";
export { createWebGpuRenderer } from "./webgpu-renderer";

export {
  YasTerminalSurface,
  isIOS,
  terminalSurfaceForInput,
} from "./YasTerminalSurface";
export type {
  YasTerminalSurfaceOptions,
  YasTerminalSurfaceHandle,
  LinkHover,
} from "./YasTerminalSurface";

export {
  YAS_SURFACE_TEXT_INPUT_EVENT,
  YasSurfaceCanvas,
  surfaceCanvasForInput,
  detectCodecSupport,
  getCodecSupport,
  getAllowedCodecSupport,
  getProbedCodecSupport,
  setAllowedCodecSupport,
  getMaxDecodeSize,
} from "./YasSurfaceCanvas";
export type {
  YasSurfaceTextInputEvent,
  YasSurfaceCanvasOptions,
  SurfaceTouchMode,
} from "./YasSurfaceCanvas";

export {
  LAYOUT_DSL_MAX_DEPTH,
  LAYOUT_DSL_MAX_PANES,
  parseDSL,
  serializeDSL,
  leafCount,
} from "./layout/dsl";
export type {
  LayoutNode,
  LayoutSplit,
  LayoutChild,
  LayoutLeaf,
} from "./layout/dsl";

export {
  enumeratePanes,
  assignSessionsToPanes,
  buildCandidateOrder,
  reconcileAssignments,
  adjustWeights,
  layoutFromDSL,
} from "./layout/tree";
export type {
  WorkspaceLayout,
  LayoutPane,
  LayoutAssignments,
} from "./layout/tree";
