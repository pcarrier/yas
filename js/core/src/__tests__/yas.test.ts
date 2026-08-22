import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  YasTransportEventMap,
  YasTransportMessage,
  ConnectionStatus,
} from "../types";
import {
  YAS_CLASS_EVENT,
  YAS_CLASS_REQUEST,
  YAS_CLASS_RESULT,
  YAS_CODEC_LZ4,
  YAS_CORE_CANCEL,
  YAS_CORE_FAMILY_UPDATE,
  YAS_CORE_GOAWAY,
  YAS_CORE_HELLO,
  YAS_CORE_PING,
  YAS_CORE_SESSION_INFO,
  YAS_CORE_SESSION_UPDATE,
  YAS_CORE_SHUTDOWN,
  YAS_CLIENT_DISCONNECT,
  YAS_CLIENT_BANDWIDTH_RATES_EXTENSION,
  YAS_CLIENT_STATE,
  YAS_CLIENT_STATE_ACK,
  YAS_CLIENT_UNWATCH,
  YAS_CLIENT_WATCH,
  YAS_FAMILY_CLIENT,
  YAS_FAMILY_CHANNEL,
  YAS_FAMILY_CORE,
  YAS_FAMILY_DESKTOP,
  YAS_FAMILY_ENV,
  YAS_FAMILY_EVENTS,
  YAS_FAMILY_EXTENSION,
  YAS_FAMILY_FONT,
  YAS_FAMILY_FS,
  YAS_FAMILY_GIT,
  YAS_FAMILY_KV,
  YAS_FAMILY_LIMIT_POLICIES,
  YAS_FAMILY_LSP,
  YAS_FAMILY_MEDIA,
  YAS_FAMILY_NET,
  YAS_FAMILY_PROCESS,
  YAS_FAMILY_RELAY,
  YAS_FAMILY_SELECTION,
  YAS_FAMILY_SURFACE,
  YAS_FAMILY_TRANSFER,
  YAS_FAMILY_TERMINAL,
  YAS_EVENTS_LIMIT_MAX_RING_BYTES,
  YAS_EVENTS_LIMIT_MIN_RING_BYTES,
  YAS_FONT_DESCRIBE,
  YAS_FONT_FETCH,
  YAS_FONT_STATE,
  YAS_FONT_STATE_ACK,
  YAS_FONT_UNWATCH,
  YAS_FONT_WATCH,
  YAS_FONT_FACE_BYTES_CONTENT_KIND,
  YAS_FONT_FORMAT_TRUETYPE,
  YAS_FONT_STYLE_NORMAL,
  YAS_FONT_VERSION,
  YAS_MEDIA_CODEC_AV1,
  YAS_MEDIA_CODEC_AV1_444,
  YAS_MEDIA_CODEC_H264,
  YAS_MEDIA_CODEC_H264_444,
  YAS_MEDIA_CODEC_MJPEG,
  YAS_MEDIA_CODEC_OPUS,
  YAS_MEDIA_CODEC_PCM_F32LE,
  YAS_MEDIA_CODEC_PCM_S16LE,
  YAS_MEDIA_CODEC_VP9,
  YAS_MEDIA_FRAME,
  YAS_MEDIA_FRAME_DISCARDABLE,
  YAS_MEDIA_STREAM_CLOSED,
  YAS_MEDIA_STREAM_STATUS,
  YAS_META_COMPRESSED,
  YAS_META_SENSITIVE,
  YAS_NET_DATAGRAM,
  YAS_GOLDEN_VECTORS,
  YAS_PREFACE,
  YAS_RELAY_CONNECT,
  YAS_RELAY_DISCONNECT,
  YAS_RELAY_LIMIT_MAX_LINKS_PER_SESSION,
  YAS_RELAY_LIMIT_MAX_PENDING_CONNECTS,
  YAS_RELAY_VERSION,
  YAS_RELAY_STATE,
  YAS_RELAY_STATE_ACK,
  YAS_RELAY_UNWATCH,
  YAS_RELAY_WATCH,
  YAS_STATE_ADD,
  YAS_STATE_DELTA,
  YAS_STATE_REMOVE,
  YAS_STATE_RESET,
  YAS_STATE_SNAPSHOT_BEGIN,
  YAS_STATE_SNAPSHOT_END,
  YAS_STATE_SNAPSHOT_RECORDS,
  YAS_SELECTION_SET,
  YAS_SELECTION_GET,
  YAS_SURFACE_CODEC_AV1_V1,
  YAS_SURFACE_CODEC_H264_V1,
  YAS_SURFACE_CODEC_PNG_V1,
  YAS_SURFACE_CREATE_APP_ENDPOINT,
  YAS_SURFACE_CLOSE_VIEW,
  YAS_SURFACE_CONFIGURE_VIEW,
  YAS_SURFACE_FRAME,
  YAS_SURFACE_FRAME_KEYFRAME,
  YAS_SURFACE_RELEASE_APP_ENDPOINT,
  YAS_SURFACE_OPEN_VIEW,
  YAS_SURFACE_RESIZE,
  YAS_SURFACE_LIMIT_MAX_APP_ENDPOINTS_PER_SESSION,
  YAS_SURFACE_LIMIT_MAX_APP_ENDPOINT_LIFETIME_NS,
  YAS_STATUS_OK,
  YAS_STATUS_CANCELLED,
  YAS_STATUS_INVALID,
  YAS_STATUS_RESOURCE_EXHAUSTED,
  YAS_STATUS_STALE,
  YAS_STATUS_UNAVAILABLE,
  YAS_TRANSFER_BYTE_DATA,
  YAS_TRANSFER_CLOSE,
  YAS_TRANSFER_CREDIT,
  YAS_TRANSFER_MESSAGE_DATA,
  YAS_TRANSFER_MODE_BYTE,
  YAS_TRANSFER_MODE_MESSAGE,
  YAS_TRANSFER_RESET,
  YAS_TRANSFER_RECEIVER_TO_SENDER,
  YAS_TRANSFER_SENDER_TO_RECEIVER,
  YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION,
  YAS_TRANSFER_UPLOAD_STAGE_EXTENSION,
  YAS_TERMINAL_CLOSE_VIEW,
  YAS_TERMINAL_CLOSE,
  YAS_TERMINAL_CREATE,
  YAS_TERMINAL_CREATE_INITIAL_VIEW_EXTENSION,
  YAS_TERMINAL_CREATE_RESULT_INITIAL_VIEW_EXTENSION,
  YAS_TERMINAL_COMMAND_ARGV,
  YAS_TERMINAL_CWD_PATH,
  YAS_TERMINAL_ENVIRONMENT_EMPTY,
  YAS_TERMINAL_ENVIRONMENT_REMOVE,
  YAS_TERMINAL_ENVIRONMENT_SET,
  YAS_TERMINAL_FRAME,
  YAS_TERMINAL_FRAME_KEYFRAME,
  YAS_TERMINAL_FRAME_ACK,
  YAS_TERMINAL_FRAME_CHUNK,
  YAS_TERMINAL_GRID_CODEC_V1,
  YAS_TERMINAL_GOLDEN_FRAME_FLAGS,
  YAS_TERMINAL_OPEN_VIEW,
  YAS_TERMINAL_STATE,
  YAS_TERMINAL_STATE_ACK,
  YAS_TERMINAL_UNWATCH,
  YAS_TERMINAL_WATCH,
  YAS_KV_LIMIT_MAX_INLINE_BYTES,
  YAS_KV_LIMIT_MAX_VALUE_BYTES,
  YAS_MAX_RETAINED_INCOMING_REQUESTS,
  YasConnection,
  YasCursor,
  YasEdgeWebSocketTransport,
  YasFontClient,
  YasMediaClient,
  YAS_FONT_HARD_LIMITS,
  YasProtocolError,
  YasRelayClient,
  YAS_RELAY_HARD_LIMITS,
  YasRelayTunnelTransport,
  YasSurfaceClient,
  YasStreamFrameDecoder,
  YasTerminalClient,
  YasWriter,
  decodeSurfaceCodecPayload,
  decodeTerminalInitialViewRequest,
  decodeResultPayload,
  decodeTerminalFrame,
  decodeTerminalCopyRange,
  decodeTerminalCwdQuery,
  decodeTerminalJournal,
  decodeTerminalJournalResult,
  decodeTerminalOutput,
  decodeTerminalOutputResult,
  decodeTerminalQueryBody,
  decodeTerminalQueryResult,
  decodeTerminalRead,
  decodeTerminalSearch,
  decodeTerminalSearchResults,
  decodeTerminalStyledLines,
  decodeTerminalTextAndStyled,
  decodeTerminalWait,
  decodeStateEvent,
  decodeStateAck,
  decodeUnwatch,
  decodeTransferByteData,
  decodeTransferMessageData,
  decodeTransferCredit,
  decodeTransferClose,
  decodeTransferReset,
  decodeEnvGetResult,
  decodeEnvSnapshotBatch,
  decodeDesktopNotificationAction,
  decodeDesktopNotificationPatch,
  decodeDesktopNotificationRecord,
  decodeDesktopNotificationRemoval,
  decodeDesktopTrayAction,
  encodeDesktopFetchAsset,
  encodeDesktopNotificationAction,
  encodeDesktopNotificationPatch,
  encodeDesktopNotificationRecord,
  encodeDesktopNotificationRemoval,
  encodeDesktopTrayAction,
  decodeKvBatch,
  decodeKvEntry,
  decodeKvGetResult,
  decodeKvMutationResult,
  decodeKvOpen,
  decodeKvPut,
  decodeKvStageValueResult,
  decodeChannelAccept,
  decodeClientBandwidthRates,
  decodeChannelConnect,
  decodeChannelListen,
  decodeMediaFetchAsset,
  decodeMediaPlayoutReport,
  decodeMediaPortalReply,
  decodeMediaPortalRequest,
  decodeMediaPortalClose,
  decodeMediaPortalRecord,
  encodeMediaStreamStatus,
  decodeProcessExit,
  decodeProcessSpawn,
  decodeProcessStreamBundle,
  decodeNetDatagram,
  decodeNetDatagramStats,
  decodeNetEndpoint,
  decodeNetOpen,
  decodeFsApply,
  decodeFsApplyResult,
  decodeFsClose,
  decodeFsCommit,
  decodeFsCommitResult,
  decodeFsConflictDetail,
  decodeFsEntry,
  decodeFsFetch,
  decodeFsGrep,
  decodeFsIndex,
  decodeFsMove,
  decodeFsOpen,
  decodeFsQueryGrepFileRecord,
  decodeFsQueryGrepMatchRecord,
  decodeFsQueryPage,
  decodeFsQueryPathRecord,
  decodeFsQueryReadRecord,
  decodeFsQueryRecordBatch,
  decodeFsRead,
  decodeFsSearch,
  decodeFsStageWrite,
  decodeFsUnwatch,
  decodeFsWatch,
  decodeGitClose,
  decodeGitClosed,
  decodeGitBlameRecord,
  decodeGitCommitRecord,
  decodeGitContentRecord,
  decodeGitDiffRecord,
  decodeGitDiscoveryRecord,
  decodeGitEntityRecord,
  decodeGitFetchResult,
  decodeGitIndexEntryRecord,
  decodeGitObjectRecord,
  decodeGitFetch,
  decodeGitObjectId,
  decodeGitOpen,
  decodeGitOpenResult,
  decodeGitProgress,
  decodeGitLogPathRecord,
  decodeGitPatchBaseRecord,
  decodeGitPatchFileRecord,
  decodeGitPatchGapRecord,
  decodeGitPatchRowRecord,
  decodeGitQuery,
  decodeGitQueryCursor,
  decodeGitQueryPage,
  decodeGitQueryState,
  decodeGitReflogRecord,
  decodeGitTreeEntryRecord,
  decodeGitUnwatch,
  decodeGitUnwatchQuery,
  decodeGitWatch,
  decodeGitWatchQuery,
  decodeGitWorktreeRecord,
  decodeEventsDumpResult,
  decodeEventsRecordEvent,
  decodeEventsRecordingInfo,
  decodeEventsSetConfig,
  decodeExtensionAttemptContext,
  decodeExtensionCommandPage,
  decodeExtensionDeploy,
  decodeExtensionFollowResult,
  decodeExtensionObjectBeginResult,
  decodeExtensionOutputBatch,
  decodeExtensionRecord,
  decodeLspBufferBegin,
  decodeLspBufferBeginResult,
  decodeLspBufferClose,
  decodeLspBufferCommit,
  decodeLspBufferPut,
  decodeLspClose,
  decodeLspClosed,
  decodeLspDiagnosticRecord,
  decodeLspEditRecord,
  decodeLspHoverRecord,
  decodeLspListServers,
  decodeLspOpen,
  decodeLspOpenResult,
  decodeLspLocationRecord,
  decodeLspQuery,
  decodeLspQueryBody,
  decodeLspQueryPage,
  decodeLspRemovedEntity,
  decodeLspServerRecord,
  decodeLspSignatureRecord,
  decodeLspSymbolRecord,
  decodeLspStopServer,
  decodeLspUnwatch,
  decodeLspWatch,
  decodeLspWorkspaceSource,
  decodeSelectionDragDrop,
  decodeSurfaceRemoteInput,
  decodeSurfaceCreateAppEndpoint,
  decodeSurfaceCreateAppEndpointResult,
  decodeSurfaceReleaseAppEndpoint,
  decodeSelectionGet,
  decodeCancel,
  decodeFamilyUpdate,
  decodeGoAway,
  decodeNegotiatedCodecs,
  decodePing,
  decodePingResult,
  decodeServerHello,
  decodeSessionInfo,
  decodeSessionUpdate,
  decodeShutdown,
  decodeYasFrame,
  encodeCancel,
  encodeClientHello,
  encodeFamilyUpdate,
  encodeGoAway,
  encodeNegotiatedCodecs,
  encodePing,
  encodePingResult,
  encodeServerHello,
  encodeSessionInfo,
  encodeSessionUpdate,
  encodeShutdown,
  encodeClientDisconnect,
  encodeTerminalCopyRange,
  encodeTerminalCwdQuery,
  encodeTerminalJournal,
  encodeTerminalJournalResult,
  encodeTerminalOutput,
  encodeTerminalOutputResult,
  encodeTerminalQueryBody,
  encodeTerminalRead,
  encodeTerminalSearch,
  encodeTerminalSearchResults,
  encodeTerminalStyledLines,
  encodeTerminalTextAndStyled,
  encodeTerminalWait,
  encodeEnvGet,
  encodeEnvGetResult,
  encodeEnvSnapshotBatch,
  encodeKvBatch,
  encodeKvEntry,
  encodeKvGetResult,
  encodeKvMutationResult,
  encodeKvOpen,
  encodeKvPut,
  encodeKvStageValueResult,
  encodeKvWatch,
  encodeChannelAccept,
  encodeClientBandwidthRates,
  encodeChannelConnect,
  encodeChannelListen,
  encodeMediaFetchAsset,
  encodeMediaPlayoutReport,
  encodeMediaPortalReply,
  encodeMediaPortalRequest,
  encodeMediaPortalClose,
  encodeMediaPortalRecord,
  encodeProcessExit,
  encodeProcessSpawn,
  encodeProcessStreamBundle,
  encodeNetDatagram,
  encodeNetDatagramStats,
  encodeNetEndpoint,
  encodeNetOpen,
  encodeFsApply,
  encodeFsApplyResult,
  encodeFsClose,
  encodeFsCommit,
  encodeFsCommitResult,
  encodeFsConflictDetail,
  encodeFsEntry,
  encodeFsFetch,
  encodeFsGrep,
  encodeFsIndex,
  encodeFsMove,
  encodeFsOpen,
  encodeFsQueryGrepFileRecord,
  encodeFsQueryGrepMatchRecord,
  encodeFsQueryPage,
  encodeFsQueryPathRecord,
  encodeFsQueryReadRecord,
  encodeFsQueryRecordBatch,
  encodeFsRead,
  encodeFsSearch,
  encodeFsStageWrite,
  encodeFsUnwatch,
  encodeFsWatch,
  encodeGitClose,
  encodeGitClosed,
  encodeGitBlameRecord,
  encodeGitCommitRecord,
  encodeGitContentRecord,
  encodeGitDiffRecord,
  encodeGitDiscoveryRecord,
  encodeGitEntityRecord,
  encodeGitFetchResult,
  encodeGitIndexEntryRecord,
  encodeGitObjectRecord,
  encodeGitFetch,
  encodeGitObjectId,
  encodeGitOpen,
  encodeGitOpenResult,
  encodeGitProgress,
  encodeGitLogPathRecord,
  encodeGitPatchBaseRecord,
  encodeGitPatchFileRecord,
  encodeGitPatchGapRecord,
  encodeGitPatchRowRecord,
  encodeGitQuery,
  encodeGitQueryCursor,
  encodeGitQueryPage,
  encodeGitQueryState,
  encodeGitReflogRecord,
  encodeGitTreeEntryRecord,
  encodeGitUnwatch,
  encodeGitUnwatchQuery,
  encodeGitWatch,
  encodeGitWatchQuery,
  encodeGitWorktreeRecord,
  encodeEventsDumpResult,
  encodeEventsRecordEvent,
  encodeEventsRecordingInfo,
  encodeEventsSetConfig,
  encodeExtensionAttemptContext,
  encodeExtensionCommandPage,
  encodeExtensionDeploy,
  encodeExtensionFollowResult,
  encodeExtensionObjectBeginResult,
  encodeExtensionOutputBatch,
  encodeExtensionRecord,
  encodeLspBufferBegin,
  encodeLspBufferBeginResult,
  encodeLspBufferClose,
  encodeLspBufferCommit,
  encodeLspBufferPut,
  encodeLspClose,
  encodeLspClosed,
  encodeLspDiagnosticRecord,
  encodeLspEditRecord,
  encodeLspHoverRecord,
  encodeLspListServers,
  encodeLspOpen,
  encodeLspOpenResult,
  encodeLspLocationRecord,
  encodeLspQuery,
  encodeLspQueryBody,
  encodeLspQueryPage,
  encodeLspRemovedEntity,
  encodeLspServerRecord,
  encodeLspSignatureRecord,
  encodeLspSymbolRecord,
  encodeLspStopServer,
  encodeLspUnwatch,
  encodeLspWatch,
  encodeLspWorkspaceSource,
  encodeExtensions,
  fontLimitsExtensions,
  encodeResultPayload,
  encodeSurfaceOpenView,
  encodeSurfaceCreateAppEndpoint,
  encodeSurfaceCreateAppEndpointResult,
  encodeSurfaceReleaseAppEndpoint,
  encodeSurfaceFrame,
  encodeSurfaceCodecPayload,
  encodeSurfaceRemoteInput,
  encodeSelectionDragDrop,
  encodeSelectionGet,
  encodeTransferDescriptor,
  encodeTransferByteData,
  encodeTransferMessageData,
  encodeTransferCredit,
  encodeTransferClose,
  encodeTransferReset,
  encodeTerminalCreate,
  encodeTerminalInitialViewRequest,
  encodeTypedRecord,
  encodeWatch,
  encodeStateAck,
  encodeUnwatch,
  encodeYasFrame,
  frameForByteStream,
  equalBytes,
  relayLimitsExtensions,
  validateEventsCodecV1,
  validateMediaCodecPayload,
  validateTerminalGridCodecPayload,
  transfersFor,
  type YasConnectionOptions,
  type YasFontFace,
  type YasRelayRoute,
  type YasTransport,
  type YasTransferDescriptor,
} from "../yas";
import {
  YAS_BROWSER_RECEIVE_MAX_BUFFERED,
  yasBrowserConnectionOptions,
} from "../yas/defaults";

class YasMockTransport implements YasTransport {
  status: ConnectionStatus = "connected";
  authRejected = false;
  lastError: string | null = null;
  readonly sentDatagrams: Uint8Array[] = [];
  sent: Uint8Array[] = [];
  private messages = new Set<(message: YasTransportMessage) => void>();
  private datagrams = new Set<(message: YasTransportMessage) => void>();
  private statuses = new Set<(status: ConnectionStatus) => void>();

  constructor(readonly maxDatagramSize = 0) {}

  connect(): void {}
  send(data: Uint8Array): void {
    this.sent.push(new Uint8Array(data));
  }
  sendDatagram(data: Uint8Array): void {
    if (!this.maxDatagramSize) throw new Error("datagram path unavailable");
    this.sentDatagrams.push(new Uint8Array(data));
  }
  close(): void {
    this.setStatus("closed");
  }
  addEventListener<K extends keyof YasTransportEventMap>(
    type: K,
    listener: (data: YasTransportEventMap[K]) => void,
  ): void {
    if (type === "message")
      this.messages.add(listener as (message: YasTransportMessage) => void);
    else if (type === "datagram")
      this.datagrams.add(listener as (message: YasTransportMessage) => void);
    else if (type === "statuschange")
      this.statuses.add(listener as (status: ConnectionStatus) => void);
  }
  removeEventListener<K extends keyof YasTransportEventMap>(
    type: K,
    listener: (data: YasTransportEventMap[K]) => void,
  ): void {
    if (type === "message")
      this.messages.delete(listener as (message: YasTransportMessage) => void);
    else if (type === "datagram")
      this.datagrams.delete(listener as (message: YasTransportMessage) => void);
    else if (type === "statuschange")
      this.statuses.delete(listener as (status: ConnectionStatus) => void);
  }
  push(frame: Uint8Array): void {
    for (const listener of this.messages) listener(frame);
  }
  pushDatagram(frame: Uint8Array): void {
    for (const listener of this.datagrams) listener(frame);
  }
  setStatus(status: ConnectionStatus): void {
    this.status = status;
    for (const listener of this.statuses) listener(status);
  }
}

class YasStreamMockTransport extends YasMockTransport {
  readonly yasFraming = "stream" as const;
}

const clientInstance = new Uint8Array(16).fill(0x44);

function familyDescriptor(
  family: number,
  operations: readonly [
    direction: number,
    frameClass: number,
    kind: number,
  ][] = [],
  runtimeState = 0,
  limits: readonly import("../yas").YasExtension[] = canonicalFamilyLimits(
    family,
  ),
): Uint8Array {
  const body = new YasWriter()
    .u16(family)
    .u16(1)
    .u8(runtimeState)
    .u8(0)
    .u16(operations.length);
  for (const [direction, frameClass, kind] of operations)
    body.u8(direction).u8(frameClass).u16(kind);
  body.bytes(encodeExtensions(limits));
  const bytes = body.finish();
  return new YasWriter().u32(bytes.length).bytes(bytes).finish();
}

function canonicalFamilyLimits(
  family: number,
): import("../yas").YasExtension[] {
  return (YAS_FAMILY_LIMIT_POLICIES[family] ?? []).map(
    ([tag, width, , , hardMax]) => ({
      tag,
      value:
        width === 4
          ? new YasWriter().u32(Number(hardMax)).finish()
          : new YasWriter().u64(hardMax).finish(),
    }),
  );
}

function defaultFamilyDescriptors(): Uint8Array[] {
  return [
    familyDescriptor(YAS_FAMILY_CORE, [
      [3, YAS_CLASS_REQUEST, YAS_CORE_PING],
      [3, YAS_CLASS_REQUEST, YAS_CORE_CANCEL],
      [1, YAS_CLASS_REQUEST, YAS_CORE_SESSION_INFO],
      [1, YAS_CLASS_REQUEST, YAS_CORE_SHUTDOWN],
      [2, YAS_CLASS_EVENT, YAS_CORE_GOAWAY],
      [2, YAS_CLASS_EVENT, YAS_CORE_SESSION_UPDATE],
      [2, YAS_CLASS_EVENT, YAS_CORE_FAMILY_UPDATE],
    ]),
    familyDescriptor(YAS_FAMILY_TRANSFER, [
      [3, YAS_CLASS_EVENT, YAS_TRANSFER_BYTE_DATA],
      [3, YAS_CLASS_EVENT, YAS_TRANSFER_MESSAGE_DATA],
      [3, YAS_CLASS_EVENT, YAS_TRANSFER_CREDIT],
      [3, YAS_CLASS_EVENT, YAS_TRANSFER_CLOSE],
      [3, YAS_CLASS_EVENT, YAS_TRANSFER_RESET],
    ]),
    familyDescriptor(
      YAS_FAMILY_RELAY,
      [
        [1, YAS_CLASS_REQUEST, YAS_RELAY_WATCH],
        [1, YAS_CLASS_REQUEST, YAS_RELAY_UNWATCH],
        [1, YAS_CLASS_REQUEST, YAS_RELAY_CONNECT],
        [1, YAS_CLASS_REQUEST, YAS_RELAY_DISCONNECT],
        [2, YAS_CLASS_EVENT, YAS_RELAY_STATE],
        [1, YAS_CLASS_EVENT, YAS_RELAY_STATE_ACK],
      ],
      0,
      relayLimitsExtensions(YAS_RELAY_HARD_LIMITS),
    ),
    familyDescriptor(YAS_FAMILY_TERMINAL, [
      [1, YAS_CLASS_REQUEST, YAS_TERMINAL_WATCH],
      [1, YAS_CLASS_REQUEST, YAS_TERMINAL_UNWATCH],
      [1, YAS_CLASS_REQUEST, YAS_TERMINAL_OPEN_VIEW],
      [1, YAS_CLASS_REQUEST, YAS_TERMINAL_CLOSE_VIEW],
      [2, YAS_CLASS_EVENT, YAS_TERMINAL_STATE],
      [1, YAS_CLASS_EVENT, YAS_TERMINAL_STATE_ACK],
      [2, YAS_CLASS_EVENT, YAS_TERMINAL_FRAME],
      [2, YAS_CLASS_EVENT, YAS_TERMINAL_FRAME_CHUNK],
      [1, YAS_CLASS_EVENT, YAS_TERMINAL_FRAME_ACK],
    ]),
    familyDescriptor(YAS_FAMILY_CLIENT, [
      [1, YAS_CLASS_REQUEST, YAS_CLIENT_WATCH],
      [1, YAS_CLASS_REQUEST, YAS_CLIENT_UNWATCH],
      [1, YAS_CLASS_REQUEST, YAS_CLIENT_DISCONNECT],
      [2, YAS_CLASS_EVENT, YAS_CLIENT_STATE],
      [1, YAS_CLASS_EVENT, YAS_CLIENT_STATE_ACK],
    ]),
    familyDescriptor(
      YAS_FAMILY_FONT,
      [
        [1, YAS_CLASS_REQUEST, YAS_FONT_WATCH],
        [1, YAS_CLASS_REQUEST, YAS_FONT_UNWATCH],
        [1, YAS_CLASS_REQUEST, YAS_FONT_DESCRIBE],
        [1, YAS_CLASS_REQUEST, YAS_FONT_FETCH],
        [2, YAS_CLASS_EVENT, YAS_FONT_STATE],
        [1, YAS_CLASS_EVENT, YAS_FONT_STATE_ACK],
      ],
      0,
      fontLimitsExtensions(YAS_FONT_HARD_LIMITS),
    ),
  ];
}

function serverHello(
  descriptors = defaultFamilyDescriptors(),
  catalogRevision = 1n,
  receiveMaxDatagram = 0,
): Uint8Array {
  return new YasWriter()
    .u16(0)
    .u16(0)
    .bytes(new Uint8Array(16).fill(1))
    .bytes(new Uint8Array(16).fill(2))
    .u32(1024 * 1024)
    .u32(4 * 1024 * 1024)
    .u32(receiveMaxDatagram)
    .u64(16n * 1024n * 1024n)
    .u64(123n)
    .u64(catalogRevision)
    .utf8U16("home")
    .utf8U16("test")
    .u16(descriptors.length)
    .bytes(concat(descriptors))
    .bytes(encodeExtensions())
    .finish();
}

const connectionOptions: YasConnectionOptions = {
  clientInstance,
  clientName: "test",
  clientRelease: "test",
  families: [
    { family: YAS_FAMILY_TRANSFER, versions: [1], required: true },
    { family: YAS_FAMILY_RELAY, versions: [1] },
    { family: YAS_FAMILY_TERMINAL, versions: [1] },
    { family: YAS_FAMILY_CLIENT, versions: [1] },
    { family: YAS_FAMILY_FONT, versions: [1] },
  ],
};

async function connected(options: Partial<YasConnectionOptions> = {}): Promise<{
  transport: YasMockTransport;
  connection: YasConnection;
}> {
  const transport = new YasMockTransport();
  const connection = new YasConnection(transport, {
    ...connectionOptions,
    ...options,
  });
  const ready = connection.connect();
  expect(transport.sent).toHaveLength(2);
  expect(equalBytes(transport.sent[0]!, YAS_PREFACE)).toBe(true);
  const helloRequest = decodeYasFrame(transport.sent[1]!);
  expect(helloRequest).toMatchObject({
    family: YAS_FAMILY_CORE,
    kind: YAS_CORE_HELLO,
    class: YAS_CLASS_REQUEST,
    requestId: 1,
  });
  pushResult(transport, helloRequest, serverHello());
  await ready;
  return { transport, connection };
}

async function connectedWithSelection(
  options: Partial<YasConnectionOptions> = {},
): Promise<{
  transport: YasMockTransport;
  connection: YasConnection;
}> {
  const transport = new YasMockTransport();
  const connection = new YasConnection(transport, {
    ...connectionOptions,
    ...options,
    families: [
      { family: YAS_FAMILY_TRANSFER, versions: [1], required: true },
      { family: YAS_FAMILY_SELECTION, versions: [1], required: true },
    ],
  });
  const ready = connection.connect();
  const helloRequest = decodeYasFrame(transport.sent[1]!);
  pushResult(
    transport,
    helloRequest,
    serverHello([
      defaultFamilyDescriptors()[0]!,
      defaultFamilyDescriptors()[1]!,
      familyDescriptor(YAS_FAMILY_SELECTION, [
        [3, YAS_CLASS_REQUEST, YAS_SELECTION_GET],
      ]),
    ]),
  );
  await ready;
  return { transport, connection };
}

async function connectedWithMedia(): Promise<{
  transport: YasMockTransport;
  connection: YasConnection;
}> {
  const transport = new YasMockTransport(65_536);
  const connection = new YasConnection(transport, {
    ...connectionOptions,
    families: [
      { family: YAS_FAMILY_TRANSFER, versions: [1], required: true },
      { family: YAS_FAMILY_MEDIA, versions: [1], required: true },
    ],
  });
  const ready = connection.connect();
  const helloRequest = decodeYasFrame(transport.sent[1]!);
  pushResult(
    transport,
    helloRequest,
    serverHello(
      [
        defaultFamilyDescriptors()[0]!,
        defaultFamilyDescriptors()[1]!,
        familyDescriptor(YAS_FAMILY_MEDIA, [
          [3, YAS_CLASS_EVENT, YAS_MEDIA_FRAME],
          [2, YAS_CLASS_EVENT, YAS_MEDIA_STREAM_STATUS],
        ]),
      ],
      1n,
      65_536,
    ),
  );
  await ready;
  return { transport, connection };
}

async function openedSurfaceView(maxInflightFrames = 1) {
  const transport = new YasMockTransport();
  const connection = new YasConnection(transport, {
    ...connectionOptions,
    families: [
      { family: YAS_FAMILY_TRANSFER, versions: [1], required: true },
      { family: YAS_FAMILY_SURFACE, versions: [1], required: true },
    ],
  });
  const ready = connection.connect();
  pushResult(
    transport,
    lastRequest(transport),
    serverHello([
      defaultFamilyDescriptors()[0]!,
      defaultFamilyDescriptors()[1]!,
      familyDescriptor(
        YAS_FAMILY_SURFACE,
        [
          [1, YAS_CLASS_REQUEST, YAS_SURFACE_OPEN_VIEW],
          [1, YAS_CLASS_REQUEST, YAS_SURFACE_CONFIGURE_VIEW],
          [1, YAS_CLASS_REQUEST, YAS_SURFACE_CLOSE_VIEW],
          [1, YAS_CLASS_REQUEST, YAS_SURFACE_RESIZE],
        ],
        0,
        canonicalFamilyLimits(YAS_FAMILY_SURFACE),
      ),
    ]),
  );
  await ready;
  const surface = new YasSurfaceClient(connection);
  const opening = surface.openView({
    surfaceHandle: 1n,
    width: 640,
    height: 480,
    maxFps: 60,
    decoderCapacity: maxInflightFrames,
    codecVersions: [YAS_SURFACE_CODEC_PNG_V1],
  });
  const request = lastRequest(transport);
  pushResult(
    transport,
    request,
    new YasWriter()
      .u32(1)
      .u16(YAS_SURFACE_CODEC_PNG_V1)
      .u16(maxInflightFrames)
      .u32(1024 * 1024)
      .u32(4 * 1024 * 1024)
      .u64(1n)
      .bytes(encodeExtensions())
      .finish(),
  );
  return { transport, connection, surface, view: await opening };
}

async function connectedSurfaceEndpoints() {
  const transport = new YasMockTransport();
  const connection = new YasConnection(transport, {
    ...connectionOptions,
    families: [
      { family: YAS_FAMILY_TRANSFER, versions: [1], required: true },
      { family: YAS_FAMILY_SURFACE, versions: [1], required: true },
    ],
  });
  const ready = connection.connect();
  pushResult(
    transport,
    lastRequest(transport),
    serverHello([
      defaultFamilyDescriptors()[0]!,
      defaultFamilyDescriptors()[1]!,
      familyDescriptor(
        YAS_FAMILY_SURFACE,
        [
          [1, YAS_CLASS_REQUEST, YAS_SURFACE_CREATE_APP_ENDPOINT],
          [1, YAS_CLASS_REQUEST, YAS_SURFACE_RELEASE_APP_ENDPOINT],
        ],
        0,
        canonicalFamilyLimits(YAS_FAMILY_SURFACE),
      ),
    ]),
  );
  await ready;
  return { transport, connection, surface: new YasSurfaceClient(connection) };
}

function pushResult(
  transport: YasMockTransport,
  request: ReturnType<typeof decodeYasFrame>,
  body: Uint8Array,
): void {
  transport.push(
    encodeYasFrame({
      family: request.family,
      kind: request.kind,
      class: YAS_CLASS_RESULT,
      requestId: request.requestId,
      payload: encodeResultPayload(YAS_STATUS_OK, body),
    }),
  );
}

function lastRequest(
  transport: YasMockTransport,
): ReturnType<typeof decodeYasFrame> {
  const frame = decodeYasFrame(transport.sent.at(-1)!);
  expect(frame.class).toBe(YAS_CLASS_REQUEST);
  return frame;
}

function decodeOnlyStreamFrame(
  bytes: Uint8Array,
  skip = 0,
): ReturnType<typeof decodeYasFrame> {
  const frames = new YasStreamFrameDecoder().push(bytes.subarray(skip));
  expect(frames).toHaveLength(1);
  return decodeYasFrame(frames[0]!);
}

function lastStreamRequest(
  transport: YasStreamMockTransport,
): ReturnType<typeof decodeYasFrame> {
  const frame = decodeOnlyStreamFrame(transport.sent.at(-1)!);
  expect(frame.class).toBe(YAS_CLASS_REQUEST);
  return frame;
}

function pushStreamResult(
  transport: YasStreamMockTransport,
  request: ReturnType<typeof decodeYasFrame>,
  body: Uint8Array,
  status = YAS_STATUS_OK,
  sensitive = false,
): void {
  transport.push(
    frameForByteStream(
      encodeYasFrame({
        family: request.family,
        kind: request.kind,
        class: YAS_CLASS_RESULT,
        requestId: request.requestId,
        sensitive,
        payload: encodeResultPayload(status, body),
      }),
    ),
  );
}

async function connectedStream(
  families: YasConnectionOptions["families"],
  descriptors: readonly Uint8Array[],
) {
  const transport = new YasStreamMockTransport();
  const connection = new YasConnection(transport, {
    ...connectionOptions,
    families,
  });
  const ready = connection.connect();
  const helloRequest = decodeOnlyStreamFrame(
    transport.sent[0]!,
    YAS_PREFACE.length,
  );
  pushStreamResult(transport, helloRequest, serverHello(descriptors));
  await ready;
  return { transport, connection };
}

function pushEvent(
  transport: YasMockTransport,
  family: number,
  kind: number,
  payload: Uint8Array,
  sensitive = false,
): void {
  transport.push(
    encodeYasFrame({
      family,
      kind,
      class: YAS_CLASS_EVENT,
      sensitive,
      payload,
    }),
  );
}

function concat(parts: readonly Uint8Array[]): Uint8Array {
  const output = new Uint8Array(
    parts.reduce((sum, part) => sum + part.length, 0),
  );
  let offset = 0;
  for (const part of parts) {
    output.set(part, offset);
    offset += part.length;
  }
  return output;
}

function stateEvent(
  subscriptionId: number,
  phase: number,
  from: bigint,
  to: bigint,
  records: readonly Uint8Array[] = [],
): Uint8Array {
  return new YasWriter()
    .u32(subscriptionId)
    .u8(phase)
    .u8(0)
    .u16(0)
    .u64(from)
    .u64(to)
    .u16(records.length)
    .bytes(concat(records))
    .finish();
}

function transferDescriptor(
  transferId: number,
  direction: number,
  receiverCredit: bigint,
  senderCredit: bigint,
  contentFamily: number,
  contentKind: number,
  sensitiveContent = false,
): Uint8Array {
  return new YasWriter()
    .u32(transferId)
    .u8(YAS_TRANSFER_MODE_BYTE)
    .u8(direction)
    .u16(0)
    .u64(receiverCredit)
    .u64(senderCredit)
    .u64(0n)
    .u32(64 * 1024)
    .u16(contentFamily)
    .u16(contentKind)
    .u16(1)
    .bytes(
      encodeExtensions(
        sensitiveContent
          ? [
              {
                tag: YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION,
                required: true,
                value: new Uint8Array(0),
              },
            ]
          : [],
      ),
    )
    .finish();
}

describe("YAS v1", () => {
  it("negotiates, sends, receives, and safely drops transport datagrams", async () => {
    const transport = new YasMockTransport(65_536);
    const connection = new YasConnection(transport, {
      ...connectionOptions,
      families: [
        { family: YAS_FAMILY_TRANSFER, versions: [1], required: true },
        { family: YAS_FAMILY_NET, versions: [1], required: true },
      ],
    });
    const ready = connection.connect();
    const helloRequest = decodeYasFrame(transport.sent[1]!);
    const helloPayload = new YasCursor(helloRequest.payload);
    helloPayload.u16("minimum minor");
    helloPayload.u16("maximum minor");
    helloPayload.u32("maximum frame");
    helloPayload.u32("maximum decoded frame");
    expect(helloPayload.u32("maximum datagram")).toBe(65_536);
    pushResult(
      transport,
      helloRequest,
      serverHello(
        [
          defaultFamilyDescriptors()[0]!,
          defaultFamilyDescriptors()[1]!,
          familyDescriptor(YAS_FAMILY_NET, [
            [3, YAS_CLASS_EVENT, YAS_NET_DATAGRAM],
          ]),
        ],
        1n,
        65_536,
      ),
    );
    await ready;

    const payload = encodeNetDatagram({
      flowHandle: 7n,
      sequence: 11n,
      payload: new Uint8Array([1, 2, 3]),
    });
    const events: { datagram: boolean; payload: Uint8Array }[] = [];
    connection.onEvent(YAS_FAMILY_NET, YAS_NET_DATAGRAM, (event) =>
      events.push({ datagram: event.datagram, payload: event.payload }),
    );
    expect(
      connection.sendDatagramEvent(YAS_FAMILY_NET, YAS_NET_DATAGRAM, payload),
    ).toBe(true);
    expect(transport.sentDatagrams).toHaveLength(1);
    const valid = transport.sentDatagrams[0]!;
    expect(valid.length).toBeLessThanOrEqual(65_536);
    transport.pushDatagram(valid);
    expect(events).toEqual([{ datagram: true, payload }]);

    const missingSensitive = new Uint8Array(valid);
    missingSensitive[4] &= ~YAS_META_SENSITIVE;
    transport.pushDatagram(missingSensitive);
    const compressed = new Uint8Array(valid);
    compressed[4] |= YAS_META_COMPRESSED;
    transport.pushDatagram(compressed);
    transport.pushDatagram(
      encodeYasFrame({
        family: YAS_FAMILY_CORE,
        kind: YAS_CORE_GOAWAY,
        class: YAS_CLASS_EVENT,
        payload: new Uint8Array(0),
      }),
    );
    transport.pushDatagram(new Uint8Array(65_537));

    expect(connection.datagramCounters).toEqual({
      received: 5,
      delivered: 1,
      dropped: 4,
    });
    expect(transport.status).toBe("connected");
  });

  it("drops discardable Media datagrams after reliable final status", async () => {
    const { transport, connection } = await connectedWithMedia();
    const media = new YasMediaClient(connection);
    const frames: bigint[] = [];
    const statuses: number[] = [];
    media.onFrame((frame) => frames.push(frame.sequence));
    media.onStreamStatus((status) => statuses.push(status.status));

    const frame = (sequence: bigint) => ({
      streamHandle: 7n,
      sequence,
      captureTime: sequence,
      presentationTime: sequence,
      codecVersion: YAS_MEDIA_CODEC_OPUS,
      flags: YAS_MEDIA_FRAME_DISCARDABLE,
      fragmentIndex: 0,
      fragmentCount: 1,
      completeLength: 1,
      payload: new Uint8Array([Number(sequence)]),
    });
    media.sendFrame(frame(1n));
    transport.pushDatagram(transport.sentDatagrams.at(-1)!);
    expect(frames).toEqual([1n]);

    pushEvent(
      transport,
      YAS_FAMILY_MEDIA,
      YAS_MEDIA_STREAM_STATUS,
      encodeMediaStreamStatus({
        streamHandle: 7n,
        revision: 1n,
        status: YAS_MEDIA_STREAM_CLOSED,
        flags: 0,
        codecConfig: new Uint8Array(0),
        extensions: [],
      }),
      true,
    );
    expect(statuses).toEqual([YAS_MEDIA_STREAM_CLOSED]);

    media.sendFrame(frame(2n));
    transport.pushDatagram(transport.sentDatagrams.at(-1)!);
    expect(frames).toEqual([1n]);
    expect(transport.status).toBe("connected");
  });

  it("round-trips a client audio/video playout report", () => {
    const report = {
      streamHandle: 7n,
      consumedSequence: 41n,
      audioVideoDelayNs: 87_000_000n,
    };
    expect(decodeMediaPlayoutReport(encodeMediaPlayoutReport(report))).toEqual(
      report,
    );
  });

  it("advances the anchored server monotonic clock after HELLO", async () => {
    vi.useFakeTimers();
    try {
      const { connection } = await connected();
      expect(connection.estimatedServerMonotonicNs()).toBe(123n);
      await vi.advanceTimersByTimeAsync(2_500);
      expect(connection.estimatedServerMonotonicNs()).toBe(2_500_000_123n);
    } finally {
      vi.useRealTimers();
    }
  });

  it("encodes the normative preface and class-specific frame headers", () => {
    expect(Array.from(YAS_PREFACE)).toEqual([
      0x59, 0x41, 0x53, 0, 1, 0, 0x0d, 0x0a,
    ]);
    expect(hex(YAS_PREFACE)).toBe(
      YAS_GOLDEN_VECTORS.vectors.find((vector) => vector.name === "preface")!
        .hex,
    );
    const generatedHelloHeader = YAS_GOLDEN_VECTORS.vectors.find(
      (vector) => vector.name === "yas.core.request.hello.header",
    )!;
    expect(
      hex(
        encodeYasFrame({
          family: YAS_FAMILY_CORE,
          kind: YAS_CORE_HELLO,
          class: YAS_CLASS_REQUEST,
          requestId: 1,
          sensitive: false,
          payload: new Uint8Array(0),
        }),
      ),
    ).toBe(generatedHelloHeader.hex);
    const request = encodeYasFrame({
      family: YAS_FAMILY_FONT,
      kind: YAS_FONT_FETCH,
      class: YAS_CLASS_REQUEST,
      requestId: 0x1020_3040,
      sensitive: true,
      payload: new Uint8Array([9]),
    });
    expect(decodeYasFrame(request)).toMatchObject({
      family: YAS_FAMILY_FONT,
      kind: YAS_FONT_FETCH,
      class: YAS_CLASS_REQUEST,
      requestId: 0x1020_3040,
      sensitive: true,
    });
    for (let length = 0; length < 9; length++)
      expect(() => decodeYasFrame(request.subarray(0, length))).toThrow(
        YasProtocolError,
      );
  });

  it("matches every generated Rust/TypeScript full-payload vector", async () => {
    const consumed = new Set<string>();
    const matches = (name: string, bytes: Uint8Array) => {
      consumed.add(name);
      expect(hex(bytes)).toBe(vector(name));
    };
    matches(
      "core.client_hello.payload",
      encodeClientHello({
        minMinor: 0,
        maxMinor: 0,
        receiveMaxFrame: 1024 * 1024,
        receiveMaxDecoded: 4 * 1024 * 1024,
        receiveMaxDatagram: 0,
        receiveMaxBuffered: 16n * 1024n * 1024n,
        clientInstance: new Uint8Array(16),
        clientName: "web",
        clientRelease: "1",
        families: [
          {
            family: YAS_FAMILY_RELAY,
            versions: [YAS_RELAY_VERSION],
            required: true,
          },
        ],
        codecs: [YAS_CODEC_LZ4],
      }),
    );
    matches("core.result.ok_empty.payload", encodeResultPayload(YAS_STATUS_OK));
    const negotiated = decodeNegotiatedCodecs(
      fromHex(vector("core.negotiated_codecs.payload")),
    );
    matches(
      "core.negotiated_codecs.payload",
      encodeNegotiatedCodecs(negotiated),
    );
    const serverHello = decodeServerHello(
      fromHex(vector("core.server_hello.payload")),
    );
    matches("core.server_hello.payload", encodeServerHello(serverHello));
    const ping = decodePing(fromHex(vector("core.ping.payload")));
    matches("core.ping.payload", encodePing(ping));
    const pingResult = decodePingResult(
      fromHex(vector("core.ping_result.payload")),
    );
    matches("core.ping_result.payload", encodePingResult(pingResult));
    const cancel = decodeCancel(fromHex(vector("core.cancel.payload")));
    matches("core.cancel.payload", encodeCancel(cancel));
    const shutdown = decodeShutdown(fromHex(vector("core.shutdown.payload")));
    matches("core.shutdown.payload", encodeShutdown(shutdown));
    const goAway = decodeGoAway(fromHex(vector("core.goaway.payload")));
    matches("core.goaway.payload", encodeGoAway(goAway));
    const sessionUpdate = decodeSessionUpdate(
      fromHex(vector("core.session_update.payload")),
    );
    matches("core.session_update.payload", encodeSessionUpdate(sessionUpdate));
    const familyUpdate = decodeFamilyUpdate(
      fromHex(vector("core.family_update.payload")),
    );
    matches("core.family_update.payload", encodeFamilyUpdate(familyUpdate));
    const sessionInfo = decodeSessionInfo(
      fromHex(vector("core.session_info.payload")),
    );
    matches("core.session_info.payload", encodeSessionInfo(sessionInfo));
    matches(
      "transfer.descriptor.payload",
      encodeTransferDescriptor({
        transferId: 2,
        mode: YAS_TRANSFER_MODE_BYTE,
        direction: YAS_TRANSFER_SENDER_TO_RECEIVER,
        flags: 0,
        receiverSendCredit: 0n,
        senderSendCredit: 4096n,
        maxItemBytes: 0n,
        maxChunkBytes: 64 * 1024,
        contentFamily: YAS_FAMILY_FONT,
        contentKind: YAS_FONT_FACE_BYTES_CONTENT_KIND,
        contentVersion: YAS_FONT_VERSION,
        extensions: [],
        maxOpenMessages: 1,
      }),
    );
    matches(
      "transfer.byte_data.payload",
      encodeTransferByteData(
        decodeTransferByteData(fromHex(vector("transfer.byte_data.payload"))),
      ),
    );
    matches(
      "transfer.message_data.payload",
      encodeTransferMessageData(
        decodeTransferMessageData(
          fromHex(vector("transfer.message_data.payload")),
        ),
      ),
    );
    matches(
      "transfer.credit.payload",
      encodeTransferCredit(
        decodeTransferCredit(fromHex(vector("transfer.credit.payload"))),
      ),
    );
    matches(
      "transfer.close.payload",
      encodeTransferClose(
        decodeTransferClose(fromHex(vector("transfer.close.payload"))),
      ),
    );
    matches(
      "transfer.reset.payload",
      encodeTransferReset(
        decodeTransferReset(fromHex(vector("transfer.reset.payload"))),
      ),
    );
    matches("state.watch.payload", encodeWatch({}, 4096n));
    matches(
      "state.unwatch.payload",
      encodeUnwatch(decodeUnwatch(fromHex(vector("state.unwatch.payload")))),
    );
    matches(
      "state.ack.payload",
      encodeStateAck(decodeStateAck(fromHex(vector("state.ack.payload")))),
    );
    matches(
      "state.delta_remove.payload",
      stateEvent(1, YAS_STATE_DELTA, 1n, 2n, [
        encodeTypedRecord({
          kind: YAS_STATE_REMOVE,
          flags: 0,
          body: new YasWriter().u64(7n).u64(3n).finish(),
        }),
      ]),
    );
    matches(
      "relay.connect.payload",
      new YasWriter()
        .u64(7n)
        .u64(3n)
        .u64(4096n)
        .u16(0)
        .u16(0)
        .bytes(encodeExtensions())
        .finish(),
    );
    matches(
      "font.fetch.payload",
      new YasWriter()
        .u64(9n)
        .bytes(new Uint8Array(32).fill(0xaa))
        .u64(4096n)
        .bytes(encodeExtensions())
        .finish(),
    );
    matches(
      "terminal.create.payload",
      encodeTerminalCreate({
        rows: 24,
        cols: 80,
        operationId: new Uint8Array(16).fill(0x11),
        launch: {
          command: {
            kind: YAS_TERMINAL_COMMAND_ARGV,
            argv: [
              new TextEncoder().encode("sh"),
              new TextEncoder().encode("-l"),
            ],
          },
          cwd: {
            kind: YAS_TERMINAL_CWD_PATH,
            path: new TextEncoder().encode("/tmp"),
          },
          environmentBase: YAS_TERMINAL_ENVIRONMENT_EMPTY,
          environment: [
            {
              key: new TextEncoder().encode("LANG"),
              kind: YAS_TERMINAL_ENVIRONMENT_SET,
              value: new TextEncoder().encode("C"),
            },
            {
              key: new TextEncoder().encode("TERM"),
              kind: YAS_TERMINAL_ENVIRONMENT_REMOVE,
            },
          ],
        },
      }),
    );
    const terminalFrameBytes = fromHex(
      vector("terminal.frame.byte_budget.payload"),
    );
    const terminalFrame = decodeTerminalFrame(terminalFrameBytes);
    expect(terminalFrame).toMatchObject({ viewId: 1, sequence: 2 });
    matches(
      "terminal.frame.byte_budget.payload",
      new YasWriter()
        .u32(terminalFrame.viewId)
        .u32(terminalFrame.sequence)
        .u16(terminalFrame.flags)
        .bytes(terminalFrame.gridPayload)
        .finish(),
    );
    matches("terminal.close_view.payload", new YasWriter().u32(7).finish());
    const queryBytes = fromHex(vector("terminal.query_inline.payload"));
    const queryBody = decodeTerminalQueryBody(queryBytes);
    matches(
      "terminal.query_inline.payload",
      encodeTerminalQueryBody(queryBody),
    );
    const query = decodeTerminalQueryResult(queryBytes);
    expect(query.nextCursor).toMatchObject({
      kind: "read",
      cursor: { a: 9n, b: 2 },
    });
    expect(Array.from(await query.bytes())).toEqual(
      Array.from(new TextEncoder().encode("hello")),
    );
    const terminalRead = decodeTerminalRead(
      fromHex(vector("terminal.read.payload")),
    );
    matches("terminal.read.payload", encodeTerminalRead(terminalRead));
    const terminalSearch = decodeTerminalSearch(
      fromHex(vector("terminal.search.payload")),
    );
    matches("terminal.search.payload", encodeTerminalSearch(terminalSearch));
    const terminalCwd = decodeTerminalCwdQuery(
      fromHex(vector("terminal.cwd.payload")),
    );
    matches("terminal.cwd.payload", encodeTerminalCwdQuery(terminalCwd));
    const terminalJournal = decodeTerminalJournal(
      fromHex(vector("terminal.journal.payload")),
    );
    matches("terminal.journal.payload", encodeTerminalJournal(terminalJournal));
    const terminalOutput = decodeTerminalOutput(
      fromHex(vector("terminal.output.payload")),
    );
    matches("terminal.output.payload", encodeTerminalOutput(terminalOutput));
    const terminalWait = decodeTerminalWait(
      fromHex(vector("terminal.wait.payload")),
    );
    matches("terminal.wait.payload", encodeTerminalWait(terminalWait));
    const terminalCopyRange = decodeTerminalCopyRange(
      fromHex(vector("terminal.copy_range.payload")),
    );
    matches(
      "terminal.copy_range.payload",
      encodeTerminalCopyRange(terminalCopyRange),
    );
    const terminalSearchResults = decodeTerminalSearchResults(
      fromHex(vector("terminal.search_results.payload")),
    );
    matches(
      "terminal.search_results.payload",
      encodeTerminalSearchResults(terminalSearchResults),
    );
    const terminalJournalResult = decodeTerminalJournalResult(
      fromHex(vector("terminal.journal_result.payload")),
    );
    matches(
      "terminal.journal_result.payload",
      encodeTerminalJournalResult(terminalJournalResult),
    );
    const terminalOutputResult = decodeTerminalOutputResult(
      fromHex(vector("terminal.output_result.payload")),
    );
    matches(
      "terminal.output_result.payload",
      encodeTerminalOutputResult(terminalOutputResult),
    );
    const terminalStyled = decodeTerminalStyledLines(
      fromHex(vector("terminal.styled_lines.payload")),
    );
    matches(
      "terminal.styled_lines.payload",
      encodeTerminalStyledLines(terminalStyled),
    );
    const terminalCombined = decodeTerminalTextAndStyled(
      fromHex(vector("terminal.text_and_styled.payload")),
    );
    matches(
      "terminal.text_and_styled.payload",
      encodeTerminalTextAndStyled(terminalCombined),
    );
    matches(
      "client.disconnect.payload",
      encodeClientDisconnect(
        new Uint8Array(16).fill(1),
        new Uint8Array(16).fill(2),
        "bye",
      ),
    );
    const surfaceCreateEndpoint = decodeSurfaceCreateAppEndpoint(
      fromHex(vector("surface.create_app_endpoint.payload")),
    );
    matches(
      "surface.create_app_endpoint.payload",
      encodeSurfaceCreateAppEndpoint(surfaceCreateEndpoint),
    );
    const surfaceCreateEndpointResult = decodeSurfaceCreateAppEndpointResult(
      fromHex(vector("surface.create_app_endpoint_result.payload")),
    );
    matches(
      "surface.create_app_endpoint_result.payload",
      encodeSurfaceCreateAppEndpointResult(surfaceCreateEndpointResult),
    );
    const surfaceReleaseEndpoint = decodeSurfaceReleaseAppEndpoint(
      fromHex(vector("surface.release_app_endpoint.payload")),
    );
    matches(
      "surface.release_app_endpoint.payload",
      encodeSurfaceReleaseAppEndpoint(surfaceReleaseEndpoint),
    );
    matches(
      "surface.open_view.payload",
      encodeSurfaceOpenView({
        surfaceHandle: 1n,
        width: 1920,
        height: 1080,
        maxFps: 60,
        decoderCapacity: 3,
        codecVersions: [1, 2],
      }),
    );
    const remoteInput = decodeSurfaceRemoteInput(
      fromHex(vector("surface.remote_input.payload")),
    );
    matches(
      "surface.remote_input.payload",
      encodeSurfaceRemoteInput(remoteInput),
    );
    const selectionGet = decodeSelectionGet(
      fromHex(vector("selection.drag_get.payload")),
    );
    matches("selection.drag_get.payload", encodeSelectionGet(selectionGet));
    const selectionDrop = decodeSelectionDragDrop(
      fromHex(vector("selection.drag_drop.payload")),
    );
    matches(
      "selection.drag_drop.payload",
      encodeSelectionDragDrop(selectionDrop),
    );
    matches(
      "desktop.fetch_asset.payload",
      encodeDesktopFetchAsset(new Uint8Array(32).fill(0xaa), 4096n),
    );
    const desktopTrayAction = decodeDesktopTrayAction(
      fromHex(vector("desktop.tray_action.payload")),
    );
    matches(
      "desktop.tray_action.payload",
      encodeDesktopTrayAction(desktopTrayAction),
    );
    const desktopNotificationAction = decodeDesktopNotificationAction(
      fromHex(vector("desktop.notification_action.payload")),
    );
    matches(
      "desktop.notification_action.payload",
      encodeDesktopNotificationAction(desktopNotificationAction),
    );
    const desktopNotificationRecord = decodeDesktopNotificationRecord(
      fromHex(vector("desktop.notification_record.payload")),
    );
    matches(
      "desktop.notification_record.payload",
      encodeDesktopNotificationRecord(desktopNotificationRecord),
    );
    const desktopNotificationPatch = decodeDesktopNotificationPatch(
      fromHex(vector("desktop.notification_patch.payload")),
    );
    matches(
      "desktop.notification_patch.payload",
      encodeDesktopNotificationPatch(desktopNotificationPatch),
    );
    const desktopNotificationRemoval = decodeDesktopNotificationRemoval(
      fromHex(vector("desktop.notification_remove.payload")),
    );
    matches(
      "desktop.notification_remove.payload",
      encodeDesktopNotificationRemoval(desktopNotificationRemoval),
    );
    const mediaFetch = decodeMediaFetchAsset(
      fromHex(vector("media.fetch_asset.payload")),
    );
    matches(
      "media.fetch_asset.payload",
      encodeMediaFetchAsset(
        mediaFetch.contentHash,
        mediaFetch.initialReceiveCredit,
        mediaFetch.extensions,
      ),
    );
    for (const name of [
      "media.portal_access_request.payload",
      "media.portal_screencast_request.payload",
    ] as const) {
      const request = decodeMediaPortalRequest(fromHex(vector(name)));
      matches(name, encodeMediaPortalRequest(request));
    }
    for (const name of [
      "media.portal_access_reply.payload",
      "media.portal_screencast_reply.payload",
    ] as const) {
      const reply = decodeMediaPortalReply(fromHex(vector(name)));
      matches(name, encodeMediaPortalReply(reply));
    }
    const portalClose = decodeMediaPortalClose(
      fromHex(vector("media.portal_close.payload")),
    );
    matches("media.portal_close.payload", encodeMediaPortalClose(portalClose));
    const portalGranted = decodeMediaPortalRecord(
      fromHex(vector("media.portal_granted.payload")),
    );
    matches(
      "media.portal_granted.payload",
      encodeMediaPortalRecord(portalGranted),
    );
    const processSpawn = decodeProcessSpawn(
      fromHex(vector("process.spawn.payload")),
    );
    matches("process.spawn.payload", encodeProcessSpawn(processSpawn));
    const processBundle = decodeProcessStreamBundle(
      fromHex(vector("process.stream_bundle.payload")),
    );
    matches(
      "process.stream_bundle.payload",
      encodeProcessStreamBundle(processBundle),
    );
    const processExit = decodeProcessExit(
      fromHex(vector("process.exit.payload")),
    );
    matches("process.exit.payload", encodeProcessExit(processExit));
    const netOpen = decodeNetOpen(fromHex(vector("net.open.payload")));
    matches("net.open.payload", encodeNetOpen(netOpen));
    const netEndpoint = decodeNetEndpoint(
      fromHex(vector("net.endpoint.payload")),
    );
    matches("net.endpoint.payload", encodeNetEndpoint(netEndpoint));
    const netDatagram = decodeNetDatagram(
      fromHex(vector("net.datagram.payload")),
    );
    matches("net.datagram.payload", encodeNetDatagram(netDatagram));
    const netStats = decodeNetDatagramStats(
      fromHex(vector("net.datagram_stats.payload")),
    );
    matches("net.datagram_stats.payload", encodeNetDatagramStats(netStats));
    const fsOpen = decodeFsOpen(fromHex(vector("fs.open.payload")));
    matches("fs.open.payload", encodeFsOpen(fsOpen));
    const fsClose = decodeFsClose(fromHex(vector("fs.close.payload")));
    matches(
      "fs.close.payload",
      encodeFsClose(fsClose.rootHandle, fsClose.extensions),
    );
    const fsWatch = decodeFsWatch(fromHex(vector("fs.watch.payload")));
    matches(
      "fs.watch.payload",
      encodeFsWatch(
        fsWatch.rootHandle,
        fsWatch.flags,
        fsWatch.settleMs,
        fsWatch.inlineMax,
        fsWatch.ignorePatterns,
        fsWatch.encodedStateWatch,
      ),
    );
    const fsUnwatch = decodeFsUnwatch(fromHex(vector("fs.unwatch.payload")));
    matches("fs.unwatch.payload", encodeFsUnwatch(fsUnwatch));
    const fsFetch = decodeFsFetch(fromHex(vector("fs.fetch.payload")));
    matches("fs.fetch.payload", encodeFsFetch(fsFetch));
    const fsRead = decodeFsRead(fromHex(vector("fs.read.payload")));
    matches("fs.read.payload", encodeFsRead(fsRead));
    const fsSearch = decodeFsSearch(fromHex(vector("fs.search.payload")));
    matches("fs.search.payload", encodeFsSearch(fsSearch));
    const fsIndex = decodeFsIndex(fromHex(vector("fs.index.payload")));
    matches("fs.index.payload", encodeFsIndex(fsIndex));
    const fsGrep = decodeFsGrep(fromHex(vector("fs.grep.payload")));
    matches("fs.grep.payload", encodeFsGrep(fsGrep));
    const fsStage = decodeFsStageWrite(
      fromHex(vector("fs.stage_write.payload")),
    );
    matches("fs.stage_write.payload", encodeFsStageWrite(fsStage));
    const fsCommit = decodeFsCommit(fromHex(vector("fs.commit.payload")));
    matches("fs.commit.payload", encodeFsCommit(fsCommit));
    const fsCommitResult = decodeFsCommitResult(
      fromHex(vector("fs.commit_result.payload")),
    );
    matches("fs.commit_result.payload", encodeFsCommitResult(fsCommitResult));
    const fsConflict = decodeFsConflictDetail(
      fromHex(vector("fs.conflict_detail.payload")),
    );
    matches("fs.conflict_detail.payload", encodeFsConflictDetail(fsConflict));
    const fsApply = decodeFsApply(fromHex(vector("fs.apply.payload")));
    matches("fs.apply.payload", encodeFsApply(fsApply));
    const fsApplyResult = decodeFsApplyResult(
      fromHex(vector("fs.apply_result.payload")),
    );
    matches("fs.apply_result.payload", encodeFsApplyResult(fsApplyResult));
    const fsEntry = decodeFsEntry(fromHex(vector("fs.entry.inline.payload")));
    matches("fs.entry.inline.payload", encodeFsEntry(fsEntry));
    const fsPage = decodeFsQueryPage(
      fromHex(vector("fs.query.inline.payload")),
    );
    matches("fs.query.inline.payload", encodeFsQueryPage(fsPage));
    const fsQueryBatch = decodeFsQueryRecordBatch(
      fromHex(vector("fs.query.batch.payload")),
    );
    matches("fs.query.batch.payload", encodeFsQueryRecordBatch(fsQueryBatch));
    const fsReadRecord = decodeFsQueryReadRecord(
      fromHex(vector("fs.query.read_record.payload")),
    );
    matches(
      "fs.query.read_record.payload",
      encodeFsQueryReadRecord(fsReadRecord),
    );
    const fsPathRecord = decodeFsQueryPathRecord(
      fromHex(vector("fs.query.path_record.payload")),
    );
    matches(
      "fs.query.path_record.payload",
      encodeFsQueryPathRecord(fsPathRecord),
    );
    const fsGrepFile = decodeFsQueryGrepFileRecord(
      fromHex(vector("fs.query.grep_file_record.payload")),
    );
    matches(
      "fs.query.grep_file_record.payload",
      encodeFsQueryGrepFileRecord(fsGrepFile),
    );
    const fsGrepMatch = decodeFsQueryGrepMatchRecord(
      fromHex(vector("fs.query.grep_match_record.payload")),
    );
    matches(
      "fs.query.grep_match_record.payload",
      encodeFsQueryGrepMatchRecord(fsGrepMatch),
    );
    const fsMove = decodeFsMove(fromHex(vector("fs.state.move.payload")));
    matches("fs.state.move.payload", encodeFsMove(fsMove));
    matches(
      "env.get.payload",
      encodeEnvGet({ initialReceiveCredit: 64n * 1024n }),
    );
    const envInline = decodeEnvGetResult(fromHex(vector("env.inline.payload")));
    matches("env.inline.payload", encodeEnvGetResult(envInline));
    const envTransfer = decodeEnvGetResult(
      fromHex(vector("env.transfer.payload")),
    );
    matches("env.transfer.payload", encodeEnvGetResult(envTransfer));
    const envBatch = decodeEnvSnapshotBatch(
      fromHex(vector("env.batch.payload")),
    );
    matches("env.batch.payload", encodeEnvSnapshotBatch(envBatch));
    const kvOpen = decodeKvOpen(fromHex(vector("kv.open.payload")));
    matches("kv.open.payload", encodeKvOpen(kvOpen));
    matches(
      "kv.watch.payload",
      encodeKvWatch({
        namespaceHandle: 1n,
        inlineMax: 1024,
        initialCredit: 4096n,
      }),
    );
    const kvEntry = decodeKvEntry(fromHex(vector("kv.entry.inline.payload")));
    matches("kv.entry.inline.payload", encodeKvEntry(kvEntry));
    const kvGet = decodeKvGetResult(fromHex(vector("kv.get.transfer.payload")));
    matches("kv.get.transfer.payload", encodeKvGetResult(kvGet));
    const kvStage = decodeKvStageValueResult(
      fromHex(vector("kv.stage_value.result.payload")),
    );
    matches("kv.stage_value.result.payload", encodeKvStageValueResult(kvStage));
    const kvPut = decodeKvPut(fromHex(vector("kv.put.inline.payload")));
    matches("kv.put.inline.payload", encodeKvPut(kvPut));
    const kvMutation = decodeKvMutationResult(
      fromHex(vector("kv.mutation_result.payload")),
    );
    matches("kv.mutation_result.payload", encodeKvMutationResult(kvMutation));
    const kvBatch = decodeKvBatch(fromHex(vector("kv.batch.payload")));
    matches("kv.batch.payload", encodeKvBatch(kvBatch));
    const channelListen = decodeChannelListen(
      fromHex(vector("channel.listen.payload")),
    );
    matches(
      "channel.listen.payload",
      encodeChannelListen(
        channelListen.name,
        channelListen.operationId,
        channelListen.metadata,
        channelListen.extensions,
      ),
    );
    const channelMaxListen = decodeChannelListen(
      fromHex(vector("channel.listen.max_metadata.payload")),
    );
    matches(
      "channel.listen.max_metadata.payload",
      encodeChannelListen(
        channelMaxListen.name,
        channelMaxListen.operationId,
        channelMaxListen.metadata,
        channelMaxListen.extensions,
      ),
    );
    const channelConnect = decodeChannelConnect(
      fromHex(vector("channel.connect.payload")),
    );
    matches(
      "channel.connect.payload",
      encodeChannelConnect(
        channelConnect.listenerHandle,
        channelConnect.generation,
        channelConnect.initialReceiveCredit,
        channelConnect.metadata,
        channelConnect.extensions,
      ),
    );
    const channelAccept = decodeChannelAccept(
      fromHex(vector("channel.accept.payload")),
    );
    matches(
      "channel.accept.payload",
      encodeChannelAccept(
        channelAccept.listenerHandle,
        channelAccept.generation,
        channelAccept.endpoint,
      ),
    );
    const clientBandwidthBytes = fromHex(
      vector("client.bandwidth_rates.payload"),
    );
    const clientBandwidth = decodeClientBandwidthRates([
      {
        tag: YAS_CLIENT_BANDWIDTH_RATES_EXTENSION,
        required: false,
        value: clientBandwidthBytes,
      },
    ])!;
    matches(
      "client.bandwidth_rates.payload",
      encodeClientBandwidthRates(clientBandwidth),
    );
    const eventsConfig = decodeEventsSetConfig(
      fromHex(vector("events.set_config.payload")),
    );
    matches("events.set_config.payload", encodeEventsSetConfig(eventsConfig));
    const eventsDump = decodeEventsDumpResult(
      fromHex(vector("events.dump_result.payload")),
    );
    matches("events.dump_result.payload", encodeEventsDumpResult(eventsDump));
    const eventsRecord = decodeEventsRecordEvent(
      fromHex(vector("events.record.payload")),
    );
    matches("events.record.payload", encodeEventsRecordEvent(eventsRecord));
    const eventsRecording = decodeEventsRecordingInfo(
      fromHex(vector("events.recording_info.payload")),
    );
    matches(
      "events.recording_info.payload",
      encodeEventsRecordingInfo(eventsRecording),
    );
    const lspOpen = decodeLspOpen(fromHex(vector("lsp.open.payload")));
    matches("lsp.open.payload", encodeLspOpen(lspOpen));
    const lspOpenAuto = decodeLspOpen(fromHex(vector("lsp.open_auto.payload")));
    matches("lsp.open_auto.payload", encodeLspOpen(lspOpenAuto));
    for (const name of [
      "lsp.open_result.payload",
      "lsp.open_result_no_backend.payload",
    ]) {
      const result = decodeLspOpenResult(fromHex(vector(name)));
      matches(name, encodeLspOpenResult(result));
    }
    const lspPlatformSource = decodeLspWorkspaceSource(
      fromHex(vector("lsp.workspace_source.platform.payload")),
    );
    matches(
      "lsp.workspace_source.platform.payload",
      encodeLspWorkspaceSource(lspPlatformSource),
    );
    const lspClosed = decodeLspClosed(fromHex(vector("lsp.closed.payload")));
    matches("lsp.closed.payload", encodeLspClosed(lspClosed));
    const lspClose = decodeLspClose(fromHex(vector("lsp.close.payload")));
    matches(
      "lsp.close.payload",
      encodeLspClose(lspClose.workspaceHandle, lspClose.extensions),
    );
    const lspWatch = decodeLspWatch(fromHex(vector("lsp.watch.payload")));
    matches(
      "lsp.watch.payload",
      encodeLspWatch(
        lspWatch.workspaceHandle,
        lspWatch.datasets,
        lspWatch.encodedStateWatch,
      ),
    );
    matches(
      "lsp.unwatch.payload",
      encodeLspUnwatch(
        decodeLspUnwatch(fromHex(vector("lsp.unwatch.payload"))),
      ),
    );
    const lspQuery = decodeLspQuery(fromHex(vector("lsp.query.payload")));
    matches("lsp.query.payload", encodeLspQuery(lspQuery));
    const lspSignatureQuery = decodeLspQueryBody(
      fromHex(vector("lsp.signature_query.payload")),
    );
    matches(
      "lsp.signature_query.payload",
      encodeLspQueryBody(lspSignatureQuery),
    );
    const lspBufferPut = decodeLspBufferPut(
      fromHex(vector("lsp.buffer_put.payload")),
    );
    matches("lsp.buffer_put.payload", encodeLspBufferPut(lspBufferPut));
    const lspBufferBegin = decodeLspBufferBegin(
      fromHex(vector("lsp.buffer_begin.payload")),
    );
    matches("lsp.buffer_begin.payload", encodeLspBufferBegin(lspBufferBegin));
    const lspBufferCommit = decodeLspBufferCommit(
      fromHex(vector("lsp.buffer_commit.payload")),
    );
    matches(
      "lsp.buffer_commit.payload",
      encodeLspBufferCommit(lspBufferCommit),
    );
    const lspBufferClose = decodeLspBufferClose(
      fromHex(vector("lsp.buffer_close.payload")),
    );
    matches("lsp.buffer_close.payload", encodeLspBufferClose(lspBufferClose));
    const lspListServers = decodeLspListServers(
      fromHex(vector("lsp.list_servers.payload")),
    );
    matches(
      "lsp.list_servers.payload",
      encodeLspListServers(
        lspListServers.workspaceHandle,
        lspListServers.extensions,
      ),
    );
    const lspStopServer = decodeLspStopServer(
      fromHex(vector("lsp.stop_server.payload")),
    );
    matches("lsp.stop_server.payload", encodeLspStopServer(lspStopServer));
    const lspBufferBeginResult = decodeLspBufferBeginResult(
      fromHex(vector("lsp.buffer_begin_result.payload")),
    );
    matches(
      "lsp.buffer_begin_result.payload",
      encodeLspBufferBeginResult(lspBufferBeginResult),
    );
    const lspQueryPage = decodeLspQueryPage(
      fromHex(vector("lsp.query_page.payload")),
    );
    matches("lsp.query_page.payload", encodeLspQueryPage(lspQueryPage));
    const lspIncompletePage = decodeLspQueryPage(
      fromHex(vector("lsp.query_page_incomplete.payload")),
    );
    matches(
      "lsp.query_page_incomplete.payload",
      encodeLspQueryPage(lspIncompletePage),
    );
    const lspLocation = decodeLspLocationRecord(
      fromHex(vector("lsp.location.payload")),
    );
    matches("lsp.location.payload", encodeLspLocationRecord(lspLocation));
    const lspHover = decodeLspHoverRecord(fromHex(vector("lsp.hover.payload")));
    matches("lsp.hover.payload", encodeLspHoverRecord(lspHover));
    const lspSymbol = decodeLspSymbolRecord(
      fromHex(vector("lsp.symbol.payload")),
    );
    matches("lsp.symbol.payload", encodeLspSymbolRecord(lspSymbol));
    const lspEdit = decodeLspEditRecord(fromHex(vector("lsp.edit.payload")));
    matches("lsp.edit.payload", encodeLspEditRecord(lspEdit));
    const lspSignature = decodeLspSignatureRecord(
      fromHex(vector("lsp.signature.payload")),
    );
    matches("lsp.signature.payload", encodeLspSignatureRecord(lspSignature));
    const lspServer = decodeLspServerRecord(
      fromHex(vector("lsp.server.payload")),
    );
    matches("lsp.server.payload", encodeLspServerRecord(lspServer));
    const lspDiagnostics = decodeLspDiagnosticRecord(
      fromHex(vector("lsp.diagnostics.payload")),
    );
    matches(
      "lsp.diagnostics.payload",
      encodeLspDiagnosticRecord(lspDiagnostics),
    );
    const lspRemoved = decodeLspRemovedEntity(
      fromHex(vector("lsp.remove.payload")),
    );
    matches("lsp.remove.payload", encodeLspRemovedEntity(lspRemoved));
    const gitObject = decodeGitObjectId(
      fromHex(vector("git.object_id.payload")),
    );
    matches("git.object_id.payload", encodeGitObjectId(gitObject));
    const gitOpen = decodeGitOpen(fromHex(vector("git.open.payload")));
    matches("git.open.payload", encodeGitOpen(gitOpen));
    const gitOpenTerminal = decodeGitOpen(
      fromHex(vector("git.open_terminal.payload")),
    );
    matches("git.open_terminal.payload", encodeGitOpen(gitOpenTerminal));
    const gitOpenResult = decodeGitOpenResult(
      fromHex(vector("git.open_result.payload")),
    );
    matches("git.open_result.payload", encodeGitOpenResult(gitOpenResult));
    const gitClose = decodeGitClose(fromHex(vector("git.close.payload")));
    matches(
      "git.close.payload",
      encodeGitClose(gitClose.repositoryHandle, gitClose.extensions),
    );
    const gitClosed = decodeGitClosed(fromHex(vector("git.closed.payload")));
    matches("git.closed.payload", encodeGitClosed(gitClosed));
    const gitWatch = decodeGitWatch(fromHex(vector("git.watch.payload")));
    matches(
      "git.watch.payload",
      encodeGitWatch(
        gitWatch.repositoryHandle,
        gitWatch.datasets,
        gitWatch.encodedStateWatch,
      ),
    );
    const gitWatchOptions = decodeGitWatch(
      fromHex(vector("git.watch_options.payload")),
    );
    matches(
      "git.watch_options.payload",
      encodeGitWatch(
        gitWatchOptions.repositoryHandle,
        gitWatchOptions.datasets,
        gitWatchOptions.encodedStateWatch,
      ),
    );
    matches(
      "git.unwatch.payload",
      encodeGitUnwatch(
        decodeGitUnwatch(fromHex(vector("git.unwatch.payload"))),
      ),
    );
    const gitQuery = decodeGitQuery(fromHex(vector("git.query.payload")));
    matches("git.query.payload", encodeGitQuery(gitQuery));
    for (const name of [
      "git.resolve_query.payload",
      "git.merge_base_query.payload",
      "git.log_query.payload",
      "git.tree_query.payload",
      "git.blob_query.payload",
      "git.index_query.payload",
      "git.discover_query.payload",
      "git.blame_query.payload",
      "git.reflog_query.payload",
      "git.worktrees_query.payload",
    ]) {
      const query = decodeGitQuery(fromHex(vector(name)));
      matches(name, encodeGitQuery(query));
    }
    const gitPatchQuery = decodeGitQuery(
      fromHex(vector("git.patch_query.payload")),
    );
    matches("git.patch_query.payload", encodeGitQuery(gitPatchQuery));
    const gitWatchQuery = decodeGitWatchQuery(
      fromHex(vector("git.watch_query.payload")),
    );
    matches(
      "git.watch_query.payload",
      encodeGitWatchQuery(
        gitWatchQuery.repositoryHandle,
        gitWatchQuery.maxRecords,
        gitWatchQuery.body,
        gitWatchQuery.encodedStateWatch,
      ),
    );
    matches(
      "git.unwatch_query.payload",
      encodeGitUnwatchQuery(
        decodeGitUnwatchQuery(fromHex(vector("git.unwatch_query.payload"))),
      ),
    );
    const gitFetch = decodeGitFetch(fromHex(vector("git.fetch.payload")));
    matches("git.fetch.payload", encodeGitFetch(gitFetch));
    const gitFetchResult = decodeGitFetchResult(
      fromHex(vector("git.fetch_result.payload")),
    );
    matches("git.fetch_result.payload", encodeGitFetchResult(gitFetchResult));
    const gitPage = decodeGitQueryPage(
      fromHex(vector("git.query_page.payload")),
    );
    matches("git.query_page.payload", encodeGitQueryPage(gitPage));
    const gitCommit = decodeGitCommitRecord(
      fromHex(vector("git.commit.payload")),
    );
    matches("git.commit.payload", encodeGitCommitRecord(gitCommit));
    const gitLogPath = decodeGitLogPathRecord(
      fromHex(vector("git.log_path.payload")),
    );
    matches("git.log_path.payload", encodeGitLogPathRecord(gitLogPath));
    const gitQueryCursor = decodeGitQueryCursor(
      fromHex(vector("git.query_cursor.payload")),
    );
    matches("git.query_cursor.payload", encodeGitQueryCursor(gitQueryCursor));
    const gitTreeEntry = decodeGitTreeEntryRecord(
      fromHex(vector("git.tree_entry.payload")),
    );
    matches("git.tree_entry.payload", encodeGitTreeEntryRecord(gitTreeEntry));
    const gitBlobContent = decodeGitContentRecord(
      fromHex(vector("git.blob_content.payload")),
      "blob",
    );
    matches("git.blob_content.payload", encodeGitContentRecord(gitBlobContent));
    const gitDiffRecord = decodeGitDiffRecord(
      fromHex(vector("git.diff_record.payload")),
    );
    matches("git.diff_record.payload", encodeGitDiffRecord(gitDiffRecord));
    const gitPatchFile = decodeGitPatchFileRecord(
      fromHex(vector("git.patch_file.payload")),
    );
    matches("git.patch_file.payload", encodeGitPatchFileRecord(gitPatchFile));
    const gitPatchRow = decodeGitPatchRowRecord(
      fromHex(vector("git.patch_row.payload")),
    );
    matches("git.patch_row.payload", encodeGitPatchRowRecord(gitPatchRow));
    const gitPatchGap = decodeGitPatchGapRecord(
      fromHex(vector("git.patch_gap.payload")),
    );
    matches("git.patch_gap.payload", encodeGitPatchGapRecord(gitPatchGap));
    const gitPatchBase = decodeGitPatchBaseRecord(
      fromHex(vector("git.patch_base.payload")),
    );
    matches("git.patch_base.payload", encodeGitPatchBaseRecord(gitPatchBase));
    const gitIndexRecord = decodeGitIndexEntryRecord(
      fromHex(vector("git.index_record.payload")),
    );
    matches(
      "git.index_record.payload",
      encodeGitIndexEntryRecord(gitIndexRecord),
    );
    const gitDiscoveryRecord = decodeGitDiscoveryRecord(
      fromHex(vector("git.discovery_record.payload")),
    );
    matches(
      "git.discovery_record.payload",
      encodeGitDiscoveryRecord(gitDiscoveryRecord),
    );
    const gitBlameRecord = decodeGitBlameRecord(
      fromHex(vector("git.blame_record.payload")),
    );
    matches("git.blame_record.payload", encodeGitBlameRecord(gitBlameRecord));
    const gitReflogRecord = decodeGitReflogRecord(
      fromHex(vector("git.reflog_record.payload")),
    );
    matches(
      "git.reflog_record.payload",
      encodeGitReflogRecord(gitReflogRecord),
    );
    const gitWorktreeRecord = decodeGitWorktreeRecord(
      fromHex(vector("git.worktree_record.payload")),
    );
    matches(
      "git.worktree_record.payload",
      encodeGitWorktreeRecord(gitWorktreeRecord),
    );
    const gitEntity = decodeGitEntityRecord(
      fromHex(vector("git.entity.payload")),
    );
    matches("git.entity.payload", encodeGitEntityRecord(gitEntity));
    const gitObjectRecord = decodeGitObjectRecord(
      fromHex(vector("git.object_record.payload")),
    );
    matches(
      "git.object_record.payload",
      encodeGitObjectRecord(gitObjectRecord),
    );
    const gitHeadEntity = decodeGitEntityRecord(
      fromHex(vector("git.entity.head.payload")),
    );
    matches("git.entity.head.payload", encodeGitEntityRecord(gitHeadEntity));
    const gitRefEntity = decodeGitEntityRecord(
      fromHex(vector("git.entity.ref.payload")),
    );
    matches("git.entity.ref.payload", encodeGitEntityRecord(gitRefEntity));
    const gitRemoteEntity = decodeGitEntityRecord(
      fromHex(vector("git.entity.remote.payload")),
    );
    matches(
      "git.entity.remote.payload",
      encodeGitEntityRecord(gitRemoteEntity),
    );
    const gitOperationEntity = decodeGitEntityRecord(
      fromHex(vector("git.entity.operation.payload")),
    );
    matches(
      "git.entity.operation.payload",
      encodeGitEntityRecord(gitOperationEntity),
    );
    const gitStatusEntity = decodeGitEntityRecord(
      fromHex(vector("git.entity.status.payload")),
    );
    matches(
      "git.entity.status.payload",
      encodeGitEntityRecord(gitStatusEntity),
    );
    for (const name of [
      "git.entity.upstream.payload",
      "git.entity.stash.payload",
      "git.entity.worktree_generation.payload",
    ]) {
      const entity = decodeGitEntityRecord(fromHex(vector(name)));
      matches(name, encodeGitEntityRecord(entity));
    }
    const gitProgress = decodeGitProgress(
      fromHex(vector("git.progress.payload")),
    );
    matches("git.progress.payload", encodeGitProgress(gitProgress));
    const gitQueryState = decodeGitQueryState(
      fromHex(vector("git.query_state.payload")),
    );
    matches("git.query_state.payload", encodeGitQueryState(gitQueryState));
    const gitQueryStateError = decodeGitQueryState(
      fromHex(vector("git.query_state_error.payload")),
    );
    matches(
      "git.query_state_error.payload",
      encodeGitQueryState(gitQueryStateError),
    );
    const extensionObjectBegin = decodeExtensionObjectBeginResult(
      fromHex(vector("extension.object_begin_result.payload")),
    );
    matches(
      "extension.object_begin_result.payload",
      encodeExtensionObjectBeginResult(extensionObjectBegin),
    );
    const extensionDeploy = decodeExtensionDeploy(
      fromHex(vector("extension.deploy.payload")),
    );
    matches("extension.deploy.payload", encodeExtensionDeploy(extensionDeploy));
    const extensionState = decodeExtensionRecord(
      fromHex(vector("extension.state.payload")),
    );
    matches("extension.state.payload", encodeExtensionRecord(extensionState));
    const extensionFollow = decodeExtensionFollowResult(
      fromHex(vector("extension.follow_result.payload")),
    );
    matches(
      "extension.follow_result.payload",
      encodeExtensionFollowResult(extensionFollow),
    );
    const extensionOutput = decodeExtensionOutputBatch(
      fromHex(vector("extension.output_batch.payload")),
    );
    matches(
      "extension.output_batch.payload",
      encodeExtensionOutputBatch(extensionOutput),
    );
    const extensionCommands = decodeExtensionCommandPage(
      fromHex(vector("extension.command_page.payload")),
    );
    matches(
      "extension.command_page.payload",
      encodeExtensionCommandPage(extensionCommands),
    );
    const extensionAttempt = decodeExtensionAttemptContext(
      fromHex(vector("extension.attempt_context.payload")),
    );
    matches(
      "extension.attempt_context.payload",
      encodeExtensionAttemptContext(extensionAttempt),
    );
    const packedEvents = fromHex(vector("packed_codec.events-v1.payload"));
    matches(
      "packed_codec.events-v1.payload",
      validateEventsCodecV1(packedEvents),
    );
    for (const [name, codec, channels] of [
      ["packed_codec.media-av1-444-v1.payload", YAS_MEDIA_CODEC_AV1_444, 1],
      ["packed_codec.media-av1-v1.payload", YAS_MEDIA_CODEC_AV1, 1],
      ["packed_codec.media-h264-444-v1.payload", YAS_MEDIA_CODEC_H264_444, 1],
      ["packed_codec.media-h264-v1.payload", YAS_MEDIA_CODEC_H264, 1],
      ["packed_codec.media-mjpeg-v1.payload", YAS_MEDIA_CODEC_MJPEG, 1],
      ["packed_codec.media-opus-v1.payload", YAS_MEDIA_CODEC_OPUS, 1],
      ["packed_codec.media-pcm-f32le-v1.payload", YAS_MEDIA_CODEC_PCM_F32LE, 1],
      ["packed_codec.media-pcm-s16le-v1.payload", YAS_MEDIA_CODEC_PCM_S16LE, 2],
      ["packed_codec.media-vp9-v1.payload", YAS_MEDIA_CODEC_VP9, 1],
    ] as const) {
      const payload = fromHex(vector(name));
      matches(name, validateMediaCodecPayload(codec, payload, channels));
    }
    for (const [name, codec] of [
      ["packed_codec.surface-av1-v1.payload", YAS_SURFACE_CODEC_AV1_V1],
      ["packed_codec.surface-h264-v1.payload", YAS_SURFACE_CODEC_H264_V1],
      ["packed_codec.surface-png-v1.payload", YAS_SURFACE_CODEC_PNG_V1],
      [
        "packed_codec.surface-av1-v1.logical_dimensions.payload",
        YAS_SURFACE_CODEC_AV1_V1,
      ],
      [
        "packed_codec.surface-h264-v1.logical_dimensions.payload",
        YAS_SURFACE_CODEC_H264_V1,
      ],
      [
        "packed_codec.surface-png-v1.logical_dimensions.payload",
        YAS_SURFACE_CODEC_PNG_V1,
      ],
    ] as const) {
      const payload = fromHex(vector(name));
      matches(
        name,
        encodeSurfaceCodecPayload(
          codec,
          decodeSurfaceCodecPayload(codec, payload),
        ),
      );
    }
    const packedTerminal = fromHex(
      vector("packed_codec.terminal-grid-v1.payload"),
    );
    validateTerminalGridCodecPayload(
      packedTerminal,
      YAS_TERMINAL_GOLDEN_FRAME_FLAGS,
    );
    matches("packed_codec.terminal-grid-v1.payload", packedTerminal);
    const fullPayloadNames = YAS_GOLDEN_VECTORS.vectors
      .map((entry) => entry.name)
      .filter((name) => name.endsWith(".payload"));
    expect([...consumed].sort()).toEqual([...fullPayloadNames].sort());
  });

  it("round-trips Surface coded and logical dimensions metadata", () => {
    const bitstream = new Uint8Array([0, 0, 0, 1, 0x65, 0x88]);
    const payload = encodeSurfaceCodecPayload(YAS_SURFACE_CODEC_H264_V1, {
      dimensions: { width: 424, height: 302 },
      logicalDimensions: { width: 400, height: 300 },
      bitstream,
    });
    expect(
      decodeSurfaceCodecPayload(YAS_SURFACE_CODEC_H264_V1, payload),
    ).toEqual({
      colorSpace: undefined,
      damage: undefined,
      dimensions: { width: 424, height: 302 },
      logicalDimensions: { width: 400, height: 300 },
      bitstream,
    });
    expect(() =>
      encodeSurfaceCodecPayload(YAS_SURFACE_CODEC_H264_V1, {
        dimensions: { width: 0, height: 302 },
        bitstream,
      }),
    ).toThrow(/Surface dimensions/);
  });

  it("rejects malformed logical dimensions and skips unknown optional metadata", () => {
    const bitstream = new Uint8Array([0, 0, 0, 1, 0x65, 0x88]);
    for (const width of [0, -1, 1.5, Infinity, 2 ** 32]) {
      expect(() =>
        encodeSurfaceCodecPayload(YAS_SURFACE_CODEC_H264_V1, {
          logicalDimensions: { width, height: 300 },
          bitstream,
        }),
      ).toThrow(/Surface dimensions/);
    }
    const payload = encodeSurfaceCodecPayload(YAS_SURFACE_CODEC_H264_V1, {
      logicalDimensions: { width: 400, height: 300 },
      bitstream,
    });
    // The extent has two nonzero u32 fields and exactly eight body bytes.
    const zeroWidth = payload.slice();
    zeroWidth.fill(0, 12, 16);
    expect(() =>
      decodeSurfaceCodecPayload(YAS_SURFACE_CODEC_H264_V1, zeroWidth),
    ).toThrow(/Surface dimensions/);
    const extraByte = payload.slice();
    extraByte[8] = 9;
    expect(() =>
      decodeSurfaceCodecPayload(YAS_SURFACE_CODEC_H264_V1, extraByte),
    ).toThrow();
    for (let end = 0; end < 20; end++)
      expect(() =>
        decodeSurfaceCodecPayload(
          YAS_SURFACE_CODEC_H264_V1,
          payload.slice(0, end),
        ),
      ).toThrow();
    const unknown = payload.slice();
    unknown[4] = 0xff;
    expect(
      decodeSurfaceCodecPayload(YAS_SURFACE_CODEC_H264_V1, unknown).bitstream,
    ).toEqual(bitstream);
    unknown[6] = 1;
    expect(() =>
      decodeSurfaceCodecPayload(YAS_SURFACE_CODEC_H264_V1, unknown),
    ).toThrow(/required Surface metadata/);
  });

  it("enforces Surface app-endpoint lifetime, live cap, and release", async () => {
    const surfaceLimits = canonicalFamilyLimits(YAS_FAMILY_SURFACE).map(
      (limit) =>
        limit.tag === YAS_SURFACE_LIMIT_MAX_APP_ENDPOINTS_PER_SESSION
          ? { ...limit, value: new YasWriter().u32(1).finish() }
          : limit.tag === YAS_SURFACE_LIMIT_MAX_APP_ENDPOINT_LIFETIME_NS
            ? {
                ...limit,
                value: new YasWriter().u64(10_000_000_000n).finish(),
              }
            : limit,
    );
    const transport = new YasMockTransport();
    const connection = new YasConnection(transport, {
      ...connectionOptions,
      families: [
        { family: YAS_FAMILY_TRANSFER, versions: [1], required: true },
        { family: YAS_FAMILY_SURFACE, versions: [1], required: true },
      ],
    });
    const ready = connection.connect();
    pushResult(
      transport,
      lastRequest(transport),
      serverHello([
        defaultFamilyDescriptors()[0]!,
        defaultFamilyDescriptors()[1]!,
        familyDescriptor(
          YAS_FAMILY_SURFACE,
          [
            [1, YAS_CLASS_REQUEST, YAS_SURFACE_CREATE_APP_ENDPOINT],
            [1, YAS_CLASS_REQUEST, YAS_SURFACE_RELEASE_APP_ENDPOINT],
          ],
          0,
          surfaceLimits,
        ),
      ]),
    );
    await ready;
    const surface = new YasSurfaceClient(connection);
    const operationId = new Uint8Array(16).fill(9);
    const create = () =>
      surface.createAppEndpoint({
        operationId,
        applicationId: "test.app",
      });

    const first = create();
    const firstRequest = lastRequest(transport);
    const sentBeforeConcurrentReplay = transport.sent.length;
    const concurrentReplay = create();
    expect(transport.sent).toHaveLength(sentBeforeConcurrentReplay);
    pushResult(
      transport,
      firstRequest,
      encodeSurfaceCreateAppEndpointResult({
        appHandle: 8n,
        expiresServerNs: 5_000_000_123n,
        environment: [],
        extensions: [],
      }),
    );
    const firstResult = await first;
    expect(await concurrentReplay).toBe(firstResult);
    const sentBeforeLiveReplay = transport.sent.length;
    expect(await create()).toBe(firstResult);
    expect(transport.sent).toHaveLength(sentBeforeLiveReplay);

    await expect(
      surface.createAppEndpoint({
        operationId,
        applicationId: "other.app",
      }),
    ).rejects.toThrow(/operation ID was reused with a different payload/);
    expect(transport.sent).toHaveLength(sentBeforeLiveReplay);
    await expect(
      surface.createAppEndpoint({
        operationId: new Uint8Array(16).fill(10),
        applicationId: "test.app",
      }),
    ).rejects.toThrow(/live-endpoint cap/);
    expect(transport.sent).toHaveLength(sentBeforeLiveReplay);

    const release = surface.releaseAppEndpoint({
      appHandle: 8n,
      operationId: new Uint8Array(16).fill(11),
    });
    pushResult(transport, lastRequest(transport), new Uint8Array());
    await release;

    const replayAfterRelease = create();
    const replayRequest = lastRequest(transport);
    transport.push(
      encodeYasFrame({
        family: replayRequest.family,
        kind: replayRequest.kind,
        class: YAS_CLASS_RESULT,
        requestId: replayRequest.requestId,
        payload: encodeResultPayload(YAS_STATUS_STALE, new Uint8Array()),
      }),
    );
    await expect(replayAfterRelease).rejects.toMatchObject({
      status: YAS_STATUS_STALE,
    });
    const sentBeforeTombstoneMismatch = transport.sent.length;
    await expect(
      surface.createAppEndpoint({
        operationId,
        applicationId: "other-after-release.app",
      }),
    ).rejects.toThrow(/operation ID was reused with a different payload/);
    expect(transport.sent).toHaveLength(sentBeforeTombstoneMismatch);

    for (let index = 0; index < 64; index++) {
      const failed = surface.createAppEndpoint({
        operationId: new Uint8Array(16).fill(32 + index),
        applicationId: `rejected-${index}.app`,
      });
      const failedRequest = lastRequest(transport);
      transport.push(
        encodeYasFrame({
          family: failedRequest.family,
          kind: failedRequest.kind,
          class: YAS_CLASS_RESULT,
          requestId: failedRequest.requestId,
          payload: encodeResultPayload(
            YAS_STATUS_RESOURCE_EXHAUSTED,
            new Uint8Array(),
          ),
        }),
      );
      await expect(failed).rejects.toMatchObject({
        status: YAS_STATUS_RESOURCE_EXHAUSTED,
      });
    }
    expect(
      (
        surface as unknown as {
          appEndpointOperations: ReadonlyMap<string, unknown>;
        }
      ).appEndpointOperations.size,
    ).toBe(1);

    const malicious = surface.createAppEndpoint({
      operationId: new Uint8Array(16).fill(12),
      applicationId: "test.app",
    });
    const maliciousRequest = lastRequest(transport);
    pushResult(
      transport,
      maliciousRequest,
      encodeSurfaceCreateAppEndpointResult({
        appHandle: 10n,
        expiresServerNs: 20_000_000_123n,
        environment: [],
        extensions: [],
      }),
    );
    await expect(malicious).rejects.toThrow(/expiry exceeds/);
    expect(transport.status).toBe("closed");
    surface.dispose();
  });

  it("tombstones Surface app-endpoint replay ownership on expiry and invalidation", async () => {
    vi.useFakeTimers();
    try {
      const expired = await connectedSurfaceEndpoints();
      const value = {
        operationId: new Uint8Array(16).fill(13),
        applicationId: "expired.app",
      };
      const creating = expired.surface.createAppEndpoint(value);
      pushResult(
        expired.transport,
        lastRequest(expired.transport),
        encodeSurfaceCreateAppEndpointResult({
          appHandle: 13n,
          expiresServerNs: 1_000_000_123n,
          environment: [],
          extensions: [],
        }),
      );
      await creating;
      await vi.advanceTimersByTimeAsync(1_500);

      const sentBeforeExpiredReplay = expired.transport.sent.length;
      const expiredReplay = expired.surface.createAppEndpoint(value);
      expect(expired.transport.sent).toHaveLength(sentBeforeExpiredReplay + 1);
      const expiredReplayRequest = lastRequest(expired.transport);
      expired.transport.push(
        encodeYasFrame({
          family: expiredReplayRequest.family,
          kind: expiredReplayRequest.kind,
          class: YAS_CLASS_RESULT,
          requestId: expiredReplayRequest.requestId,
          payload: encodeResultPayload(YAS_STATUS_STALE, new Uint8Array()),
        }),
      );
      await expect(expiredReplay).rejects.toMatchObject({
        status: YAS_STATUS_STALE,
      });
      expired.surface.dispose();
    } finally {
      vi.useRealTimers();
    }

    const invalidated = await connectedSurfaceEndpoints();
    const value = {
      operationId: new Uint8Array(16).fill(14),
      applicationId: "invalidated.app",
    };
    const creating = invalidated.surface.createAppEndpoint(value);
    pushResult(
      invalidated.transport,
      lastRequest(invalidated.transport),
      encodeSurfaceCreateAppEndpointResult({
        appHandle: 14n,
        expiresServerNs: 1_000_000_123n,
        environment: [],
        extensions: [],
      }),
    );
    await creating;

    pushEvent(
      invalidated.transport,
      YAS_FAMILY_CORE,
      YAS_CORE_FAMILY_UPDATE,
      new YasWriter()
        .u64(2n)
        .bytes(familyDescriptor(YAS_FAMILY_SURFACE, [], 2))
        .finish(),
    );
    pushEvent(
      invalidated.transport,
      YAS_FAMILY_CORE,
      YAS_CORE_FAMILY_UPDATE,
      new YasWriter()
        .u64(3n)
        .bytes(
          familyDescriptor(YAS_FAMILY_SURFACE, [
            [1, YAS_CLASS_REQUEST, YAS_SURFACE_CREATE_APP_ENDPOINT],
            [1, YAS_CLASS_REQUEST, YAS_SURFACE_RELEASE_APP_ENDPOINT],
          ]),
        )
        .finish(),
    );

    const sentBeforeInvalidatedReplay = invalidated.transport.sent.length;
    const invalidatedReplay = invalidated.surface.createAppEndpoint(value);
    expect(invalidated.transport.sent).toHaveLength(
      sentBeforeInvalidatedReplay + 1,
    );
    const invalidatedReplayRequest = lastRequest(invalidated.transport);
    invalidated.transport.push(
      encodeYasFrame({
        family: invalidatedReplayRequest.family,
        kind: invalidatedReplayRequest.kind,
        class: YAS_CLASS_RESULT,
        requestId: invalidatedReplayRequest.requestId,
        payload: encodeResultPayload(YAS_STATUS_STALE, new Uint8Array()),
      }),
    );
    await expect(invalidatedReplay).rejects.toMatchObject({
      status: YAS_STATUS_STALE,
    });
    invalidated.surface.dispose();
  });

  it("rejects Terminal views completing after family invalidation", async () => {
    const { transport, connection } = await connected();
    const terminals = new YasTerminalClient(connection);
    const opening = terminals.openView({
      terminalHandle: 1n,
      rows: 24,
      cols: 80,
      maxFps: 60,
      codecVersions: [YAS_TERMINAL_GRID_CODEC_V1],
    });
    const openRequest = lastRequest(transport);
    pushEvent(
      transport,
      YAS_FAMILY_CORE,
      YAS_CORE_FAMILY_UPDATE,
      new YasWriter()
        .u64(2n)
        .bytes(familyDescriptor(YAS_FAMILY_TERMINAL, [], 2))
        .finish(),
    );
    pushResult(
      transport,
      openRequest,
      new YasWriter()
        .u32(7)
        .u16(YAS_TERMINAL_GRID_CODEC_V1)
        .u8(2)
        .u8(0)
        .u32(64)
        .u32(128)
        .u32(1)
        .bytes(encodeExtensions())
        .finish(),
    );

    await expect(opening).rejects.toThrow(/family invalidation/);
    connection.receiveBudget
      .reserve(16n * 1024n * 1024n, 16n * 1024n * 1024n)
      .release();
    terminals.dispose();
    await expect(
      terminals.openView({
        terminalHandle: 1n,
        rows: 24,
        cols: 80,
        maxFps: 60,
        codecVersions: [YAS_TERMINAL_GRID_CODEC_V1],
      }),
    ).rejects.toThrow(/client is disposed/);
  });

  it("admits the canonical browser catalogue, Terminal, and Surface inventory", () => {
    const options = yasBrowserConnectionOptions("test");
    expect(options.receiveMaxBuffered).toBe(YAS_BROWSER_RECEIVE_MAX_BUFFERED);
    const connection = new YasConnection(new YasMockTransport(), {
      ...options,
      clientInstance,
    });
    // One 1 MiB State window per family that publishes a catalogue, a Terminal
    // and a Surface view, and headroom for the per-query windows Git, FS, and
    // LSP open on top.
    const leases = Array.from({ length: 16 }, () =>
      connection.receiveBudget.reserve(1024n * 1024n),
    );
    leases.push(connection.receiveBudget.reserve(4n * 1024n * 1024n));
    leases.push(connection.receiveBudget.reserve(4n * 1024n * 1024n));
    leases.push(connection.receiveBudget.reserve(1000n * 1024n * 1024n));
    expect(() => connection.receiveBudget.reserve(1n)).toThrow(/exhausted/);
    for (const lease of leases) lease.release();

    const mutable = connection.receiveBudget.reserveExact(4n * 1024n * 1024n);
    mutable.resizeExact(YAS_BROWSER_RECEIVE_MAX_BUFFERED);
    expect(() => connection.receiveBudget.reserve(1n)).toThrow(/exhausted/);
    mutable.resizeExact(4n * 1024n * 1024n);
    connection.receiveBudget.reserveExact(1020n * 1024n * 1024n).release();
    mutable.release();
  });

  it("transactionally grows and retains the Surface CONFIGURE_VIEW high-water lease", async () => {
    const { transport, connection, surface, view } = await openedSurfaceView();
    const grow = view.configure({
      width: 800,
      height: 600,
      maxFps: 30,
      decoderCapacity: 3,
      latencyTargetNs: 0n,
    });
    await vi.waitFor(() =>
      expect(decodeYasFrame(transport.sent.at(-1)!).kind).toBe(
        YAS_SURFACE_CONFIGURE_VIEW,
      ),
    );
    const growRequest = lastRequest(transport);
    expect(() =>
      connection.receiveBudget.reserveExact(14n * 1024n * 1024n),
    ).toThrow(/exhausted/);
    pushResult(transport, growRequest, new Uint8Array());
    await grow;
    expect(view.result.maxInflightFrames).toBe(3);

    const sentBeforeShrink = transport.sent.length;
    const shrink = view.configure({
      width: 640,
      height: 480,
      maxFps: 60,
      decoderCapacity: 1,
      latencyTargetNs: 0n,
    });
    await vi.waitFor(() =>
      expect(transport.sent.length).toBe(sentBeforeShrink + 1),
    );
    const shrinkRequest = lastRequest(transport);
    expect(() =>
      connection.receiveBudget.reserveExact(14n * 1024n * 1024n),
    ).toThrow(/exhausted/);
    pushResult(transport, shrinkRequest, new Uint8Array());
    await shrink;
    expect(view.result.maxInflightFrames).toBe(1);
    expect(() =>
      connection.receiveBudget.reserveExact(14n * 1024n * 1024n),
    ).toThrow(/exhausted/);
    surface.dispose();
    connection.receiveBudget.reserveExact(16n * 1024n * 1024n).release();
  });

  it("writes Surface RESIZE before a concurrently started CONFIGURE_VIEW", async () => {
    const { transport, surface, view } = await openedSurfaceView();
    const sent = transport.sent.length;

    const resizing = surface.resize(
      1n,
      new Uint8Array(16),
      800n << 32n,
      600n << 32n,
    );
    const configuring = view.configure({
      width: 800,
      height: 600,
      maxFps: 60,
      decoderCapacity: 1,
      latencyTargetNs: 0n,
    });

    expect(decodeYasFrame(transport.sent[sent]!).kind).toBe(YAS_SURFACE_RESIZE);
    await Promise.resolve();
    expect(decodeYasFrame(transport.sent[sent + 1]!).kind).toBe(
      YAS_SURFACE_CONFIGURE_VIEW,
    );

    const resizeRequest = decodeYasFrame(transport.sent[sent]!);
    const configureRequest = decodeYasFrame(transport.sent[sent + 1]!);
    pushResult(transport, resizeRequest, new YasWriter().u64(2n).finish());
    pushResult(transport, configureRequest, new Uint8Array());
    await Promise.all([resizing, configuring]);
    surface.dispose();
  });

  it("rejects Surface CONFIGURE_VIEW growth before sending when budget is exhausted", async () => {
    const { transport, connection, surface, view } = await openedSurfaceView();
    const remainder = connection.receiveBudget.reserveExact(
      15n * 1024n * 1024n,
    );
    const sent = transport.sent.length;

    await expect(
      view.configure({
        width: 800,
        height: 600,
        maxFps: 30,
        decoderCapacity: 2,
        latencyTargetNs: 0n,
      }),
    ).rejects.toMatchObject({ status: YAS_STATUS_RESOURCE_EXHAUSTED });
    expect(transport.sent).toHaveLength(sent);
    expect(view.result.maxInflightFrames).toBe(1);
    remainder.release();
    surface.dispose();
  });

  it("rolls back Surface CONFIGURE_VIEW growth when the server rejects it", async () => {
    const { transport, connection, surface, view } = await openedSurfaceView();
    const configure = view.configure({
      width: 800,
      height: 600,
      maxFps: 30,
      decoderCapacity: 2,
      latencyTargetNs: 0n,
    });
    await vi.waitFor(() =>
      expect(decodeYasFrame(transport.sent.at(-1)!).kind).toBe(
        YAS_SURFACE_CONFIGURE_VIEW,
      ),
    );
    const request = lastRequest(transport);
    transport.push(
      encodeYasFrame({
        family: request.family,
        kind: request.kind,
        class: YAS_CLASS_RESULT,
        requestId: request.requestId,
        payload: encodeResultPayload(YAS_STATUS_RESOURCE_EXHAUSTED),
      }),
    );

    await expect(configure).rejects.toMatchObject({
      status: YAS_STATUS_RESOURCE_EXHAUSTED,
    });
    expect(view.result.maxInflightFrames).toBe(1);
    connection.receiveBudget.reserveExact(12n * 1024n * 1024n).release();
    surface.dispose();
  });

  it("installs a Terminal view before a coalesced byte-stream FRAME", async () => {
    const { transport, connection } = await connectedStream(
      [
        { family: YAS_FAMILY_TRANSFER, versions: [1], required: true },
        { family: YAS_FAMILY_TERMINAL, versions: [1], required: true },
      ],
      [
        defaultFamilyDescriptors()[0]!,
        defaultFamilyDescriptors()[1]!,
        defaultFamilyDescriptors()[3]!,
      ],
    );
    const terminals = new YasTerminalClient(connection);
    const opening = terminals.openView({
      terminalHandle: 1n,
      rows: 24,
      cols: 80,
      maxFps: 60,
      codecVersions: [YAS_TERMINAL_GRID_CODEC_V1],
    });
    const request = lastStreamRequest(transport);
    const result = encodeYasFrame({
      family: request.family,
      kind: request.kind,
      class: YAS_CLASS_RESULT,
      requestId: request.requestId,
      payload: encodeResultPayload(
        YAS_STATUS_OK,
        new YasWriter()
          .u32(7)
          .u16(YAS_TERMINAL_GRID_CODEC_V1)
          .u8(1)
          .u8(0)
          .u32(64)
          .u32(128)
          .u32(1)
          .bytes(encodeExtensions())
          .finish(),
      ),
    });
    const frame = encodeYasFrame({
      family: YAS_FAMILY_TERMINAL,
      kind: YAS_TERMINAL_FRAME,
      class: YAS_CLASS_EVENT,
      payload: new YasWriter()
        .u32(7)
        .u32(1)
        .u16(YAS_TERMINAL_FRAME_KEYFRAME)
        .bytes(new Uint8Array([0, 0]))
        .finish(),
    });
    transport.push(
      concat([frameForByteStream(result), frameForByteStream(frame)]),
    );

    const view = await opening;
    const frames: number[] = [];
    view.subscribe((value) => frames.push(value.sequence));
    expect(frames).toEqual([1]);
    expect(transport.status).toBe("connected");
    terminals.dispose();
    const closeRequest = lastStreamRequest(transport);
    expect(closeRequest.kind).toBe(YAS_TERMINAL_CLOSE_VIEW);
    pushStreamResult(transport, closeRequest, new Uint8Array());
    const sent = transport.sent.length;
    await expect(
      terminals.create({
        rows: 24,
        cols: 80,
        operationId: new Uint8Array(16).fill(1),
        launch: {
          command: {
            kind: YAS_TERMINAL_COMMAND_ARGV,
            argv: [new TextEncoder().encode("sh")],
          },
          cwd: {
            kind: YAS_TERMINAL_CWD_PATH,
            path: new TextEncoder().encode("/tmp"),
          },
          environmentBase: YAS_TERMINAL_ENVIRONMENT_EMPTY,
          environment: [],
        },
      }),
    ).rejects.toThrow(/disposed/);
    expect(transport.sent).toHaveLength(sent);
  });

  it("installs and reconfigures a Surface view before coalesced byte-stream FRAMEs", async () => {
    const { transport, connection } = await connectedStream(
      [
        { family: YAS_FAMILY_TRANSFER, versions: [1], required: true },
        { family: YAS_FAMILY_SURFACE, versions: [1], required: true },
      ],
      [
        defaultFamilyDescriptors()[0]!,
        defaultFamilyDescriptors()[1]!,
        familyDescriptor(
          YAS_FAMILY_SURFACE,
          [
            [1, YAS_CLASS_REQUEST, YAS_SURFACE_OPEN_VIEW],
            [1, YAS_CLASS_REQUEST, YAS_SURFACE_CONFIGURE_VIEW],
            [1, YAS_CLASS_REQUEST, YAS_SURFACE_CLOSE_VIEW],
            [2, YAS_CLASS_EVENT, YAS_SURFACE_FRAME],
          ],
          0,
          canonicalFamilyLimits(YAS_FAMILY_SURFACE),
        ),
      ],
    );
    const surfaces = new YasSurfaceClient(connection);
    const opening = surfaces.openView({
      surfaceHandle: 1n,
      width: 640,
      height: 480,
      maxFps: 60,
      decoderCapacity: 1,
      codecVersions: [YAS_SURFACE_CODEC_PNG_V1],
    });
    const openRequest = lastStreamRequest(transport);
    const openResult = encodeYasFrame({
      family: openRequest.family,
      kind: openRequest.kind,
      class: YAS_CLASS_RESULT,
      requestId: openRequest.requestId,
      payload: encodeResultPayload(
        YAS_STATUS_OK,
        new YasWriter()
          .u32(9)
          .u16(YAS_SURFACE_CODEC_PNG_V1)
          .u16(1)
          .u32(64)
          .u32(128)
          .u64(1n)
          .bytes(encodeExtensions())
          .finish(),
      ),
    });
    const surfaceFrame = (sequence: bigint) =>
      encodeYasFrame({
        family: YAS_FAMILY_SURFACE,
        kind: YAS_SURFACE_FRAME,
        class: YAS_CLASS_EVENT,
        payload: encodeSurfaceFrame({
          viewId: 9,
          sequence,
          baseSequence: 0n,
          captureNs: sequence,
          presentationNs: sequence,
          flags: YAS_SURFACE_FRAME_KEYFRAME,
          codecVersion: YAS_SURFACE_CODEC_PNG_V1,
          fragmentIndex: 0,
          fragmentCount: 1,
          completeLength: 1,
          payload: new Uint8Array([Number(sequence)]),
        }),
      });
    transport.push(
      concat([
        frameForByteStream(openResult),
        frameForByteStream(surfaceFrame(1n)),
      ]),
    );
    const view = await opening;

    const configuring = view.configure({
      width: 800,
      height: 600,
      maxFps: 30,
      decoderCapacity: 2,
      latencyTargetNs: 0n,
    });
    await vi.waitFor(() =>
      expect(lastStreamRequest(transport).kind).toBe(
        YAS_SURFACE_CONFIGURE_VIEW,
      ),
    );
    const configureRequest = lastStreamRequest(transport);
    const configureResult = encodeYasFrame({
      family: configureRequest.family,
      kind: configureRequest.kind,
      class: YAS_CLASS_RESULT,
      requestId: configureRequest.requestId,
      payload: encodeResultPayload(YAS_STATUS_OK),
    });
    transport.push(
      concat([
        frameForByteStream(configureResult),
        frameForByteStream(surfaceFrame(2n)),
      ]),
    );
    await configuring;

    const frames: bigint[] = [];
    view.subscribe((value) => frames.push(value.sequence));
    expect(frames).toEqual([1n, 2n]);
    expect(view.result.maxInflightFrames).toBe(2);
    expect(transport.status).toBe("connected");
    surfaces.dispose();
    const closeRequest = lastStreamRequest(transport);
    expect(closeRequest.kind).toBe(YAS_SURFACE_CLOSE_VIEW);
    pushStreamResult(transport, closeRequest, new Uint8Array());
  });

  it("owns a typed Terminal CREATE initial view before a coalesced FRAME", async () => {
    const terminalDescriptor = familyDescriptor(
      YAS_FAMILY_TERMINAL,
      [
        [1, YAS_CLASS_REQUEST, YAS_TERMINAL_CREATE],
        [1, YAS_CLASS_REQUEST, YAS_TERMINAL_CLOSE_VIEW],
        [2, YAS_CLASS_EVENT, YAS_TERMINAL_FRAME],
      ],
      0,
      canonicalFamilyLimits(YAS_FAMILY_TERMINAL),
    );
    const { transport, connection } = await connectedStream(
      [
        { family: YAS_FAMILY_TRANSFER, versions: [1], required: true },
        { family: YAS_FAMILY_TERMINAL, versions: [1], required: true },
      ],
      [
        defaultFamilyDescriptors()[0]!,
        defaultFamilyDescriptors()[1]!,
        terminalDescriptor,
      ],
    );
    const terminals = new YasTerminalClient(connection);
    const initialView = {
      rows: 24,
      cols: 80,
      maxFps: 60,
      codecVersions: [YAS_TERMINAL_GRID_CODEC_V1],
    };
    expect(
      decodeTerminalInitialViewRequest(
        encodeTerminalInitialViewRequest(initialView),
      ),
    ).toMatchObject(initialView);
    const createRequest = {
      rows: 24,
      cols: 80,
      operationId: new Uint8Array(16).fill(3),
      launch: {
        command: {
          kind: YAS_TERMINAL_COMMAND_ARGV,
          argv: [new TextEncoder().encode("sh")],
        } as const,
        cwd: {
          kind: YAS_TERMINAL_CWD_PATH,
          path: new TextEncoder().encode("/tmp"),
        } as const,
        environmentBase: YAS_TERMINAL_ENVIRONMENT_EMPTY,
        environment: [],
      },
      initialView,
    };
    expect(() =>
      encodeTerminalCreate({
        ...createRequest,
        extensions: [
          {
            tag: YAS_TERMINAL_CREATE_INITIAL_VIEW_EXTENSION,
            value: new Uint8Array(),
          },
        ],
      }),
    ).toThrow(/duplicated/);

    const creating = terminals.create(createRequest);
    const request = lastStreamRequest(transport);
    const viewResult = new YasWriter()
      .u32(11)
      .u16(YAS_TERMINAL_GRID_CODEC_V1)
      .u8(1)
      .u8(0)
      .u32(64)
      .u32(128)
      .u32(1)
      .bytes(encodeExtensions())
      .finish();
    const createBody = new YasWriter()
      .u64(5n)
      .u64(1n)
      .u32(1)
      .u32(0)
      .bytes(
        encodeExtensions([
          {
            tag: YAS_TERMINAL_CREATE_RESULT_INITIAL_VIEW_EXTENSION,
            value: viewResult,
          },
        ]),
      )
      .finish();
    const result = encodeYasFrame({
      family: request.family,
      kind: request.kind,
      class: YAS_CLASS_RESULT,
      requestId: request.requestId,
      sensitive: true,
      payload: encodeResultPayload(YAS_STATUS_OK, createBody),
    });
    const terminalFrame = (sequence: number) =>
      encodeYasFrame({
        family: YAS_FAMILY_TERMINAL,
        kind: YAS_TERMINAL_FRAME,
        class: YAS_CLASS_EVENT,
        payload: new YasWriter()
          .u32(11)
          .u32(sequence)
          .u16(YAS_TERMINAL_FRAME_KEYFRAME)
          .bytes(new Uint8Array([0, 0]))
          .finish(),
      });
    transport.push(
      concat([
        frameForByteStream(result),
        frameForByteStream(terminalFrame(1)),
      ]),
    );
    const created = await creating;
    expect(created.initialView).toBeDefined();
    const frames: number[] = [];
    created.initialView!.subscribe((value) => frames.push(value.sequence));
    expect(frames).toEqual([1]);
    created.initialView!.recordFeedback({
      viewId: 11,
      presentedSequence: 1,
      decoderQueueDepth: 0,
      availableFrameSlots: 1,
    });

    const replay = terminals.create(createRequest);
    const replayRequest = lastStreamRequest(transport);
    pushStreamResult(
      transport,
      replayRequest,
      new Uint8Array(),
      YAS_STATUS_STALE,
      true,
    );
    await expect(replay).rejects.toMatchObject({ status: YAS_STATUS_STALE });
    transport.push(frameForByteStream(terminalFrame(2)));
    expect(frames).toEqual([1, 2]);
    expect(transport.status).toBe("connected");

    const invalidReplay = terminals.create(createRequest);
    const invalidReplayRequest = lastStreamRequest(transport);
    const sentBeforeInvalidResult = transport.sent.length;
    pushStreamResult(
      transport,
      invalidReplayRequest,
      createBody,
      YAS_STATUS_OK,
      true,
    );
    await expect(invalidReplay).rejects.toThrow(/view ID was reused/);
    expect(transport.status).toBe("closed");
    expect(transport.sent).toHaveLength(sentBeforeInvalidResult);
    connection.receiveBudget.reserveExact(16n * 1024n * 1024n).release();
    terminals.dispose();
  });

  it("releases Surface app endpoints on late completion and disposal", async () => {
    const active = await connectedSurfaceEndpoints();
    const created = active.surface.createAppEndpoint({
      operationId: new Uint8Array(16).fill(4),
      applicationId: "test.app",
    });
    const createRequest = lastRequest(active.transport);
    pushResult(
      active.transport,
      createRequest,
      encodeSurfaceCreateAppEndpointResult({
        appHandle: 8n,
        expiresServerNs: 1_000_000_123n,
        environment: [],
        extensions: [],
      }),
    );
    await created;
    active.surface.dispose();
    const activeRelease = lastRequest(active.transport);
    expect(activeRelease.kind).toBe(YAS_SURFACE_RELEASE_APP_ENDPOINT);
    pushResult(active.transport, activeRelease, new Uint8Array());
    const sent = active.transport.sent.length;
    await expect(
      active.surface.createAppEndpoint({
        operationId: new Uint8Array(16).fill(5),
        applicationId: "test.app",
      }),
    ).rejects.toThrow(/disposed/);
    expect(active.transport.sent).toHaveLength(sent);

    const late = await connectedSurfaceEndpoints();
    const completing = late.surface.createAppEndpoint({
      operationId: new Uint8Array(16).fill(6),
      applicationId: "test.app",
    });
    const lateCreate = lastRequest(late.transport);
    late.surface.dispose();
    pushResult(
      late.transport,
      lateCreate,
      encodeSurfaceCreateAppEndpointResult({
        appHandle: 9n,
        expiresServerNs: 1_000_000_123n,
        environment: [],
        extensions: [],
      }),
    );
    await expect(completing).rejects.toThrow(/after family invalidation/);
    const lateRelease = lastRequest(late.transport);
    expect(lateRelease.kind).toBe(YAS_SURFACE_RELEASE_APP_ENDPOINT);
    pushResult(late.transport, lateRelease, new Uint8Array());
    await Promise.resolve();
  });

  it("does not report a Terminal CREATE that completes after client disposal", async () => {
    const descriptor = familyDescriptor(
      YAS_FAMILY_TERMINAL,
      [
        [1, YAS_CLASS_REQUEST, YAS_TERMINAL_CREATE],
        [1, YAS_CLASS_REQUEST, YAS_TERMINAL_CLOSE],
      ],
      0,
      canonicalFamilyLimits(YAS_FAMILY_TERMINAL),
    );
    const { transport, connection } = await connectedStream(
      [
        { family: YAS_FAMILY_TRANSFER, versions: [1], required: true },
        { family: YAS_FAMILY_TERMINAL, versions: [1], required: true },
      ],
      [
        defaultFamilyDescriptors()[0]!,
        defaultFamilyDescriptors()[1]!,
        descriptor,
      ],
    );
    const terminals = new YasTerminalClient(connection);
    const creating = terminals.create({
      rows: 24,
      cols: 80,
      operationId: new Uint8Array(16).fill(7),
      launch: {
        command: {
          kind: YAS_TERMINAL_COMMAND_ARGV,
          argv: [new TextEncoder().encode("sh")],
        },
        cwd: {
          kind: YAS_TERMINAL_CWD_PATH,
          path: new TextEncoder().encode("/tmp"),
        },
        environmentBase: YAS_TERMINAL_ENVIRONMENT_EMPTY,
        environment: [],
      },
    });
    const request = lastStreamRequest(transport);
    terminals.dispose();
    pushStreamResult(
      transport,
      request,
      new YasWriter()
        .u64(6n)
        .u64(1n)
        .u32(1)
        .u32(0)
        .bytes(encodeExtensions())
        .finish(),
      YAS_STATUS_OK,
      true,
    );

    await expect(creating).rejects.toThrow(/after client disposal/);
    const cleanup = lastStreamRequest(transport);
    expect(cleanup.kind).toBe(YAS_TERMINAL_CLOSE);
    pushStreamResult(transport, cleanup, new Uint8Array(), YAS_STATUS_OK, true);
    expect(transport.status).toBe("connected");
  });

  it("closes Surface views whose OPEN_VIEW completes after disposal", async () => {
    const transport = new YasMockTransport();
    const connection = new YasConnection(transport, {
      ...connectionOptions,
      families: [
        { family: YAS_FAMILY_TRANSFER, versions: [1], required: true },
        { family: YAS_FAMILY_SURFACE, versions: [1], required: true },
      ],
    });
    const ready = connection.connect();
    pushResult(
      transport,
      lastRequest(transport),
      serverHello([
        defaultFamilyDescriptors()[0]!,
        defaultFamilyDescriptors()[1]!,
        familyDescriptor(
          YAS_FAMILY_SURFACE,
          [
            [1, YAS_CLASS_REQUEST, YAS_SURFACE_OPEN_VIEW],
            [1, YAS_CLASS_REQUEST, YAS_SURFACE_CLOSE_VIEW],
          ],
          0,
          canonicalFamilyLimits(YAS_FAMILY_SURFACE),
        ),
      ]),
    );
    await ready;
    const surface = new YasSurfaceClient(connection);
    const opening = surface.openView({
      surfaceHandle: 1n,
      width: 640,
      height: 480,
      maxFps: 60,
      decoderCapacity: 2,
      codecVersions: [YAS_SURFACE_CODEC_PNG_V1],
    });
    const openRequest = lastRequest(transport);
    surface.dispose();
    pushResult(
      transport,
      openRequest,
      new YasWriter()
        .u32(9)
        .u16(YAS_SURFACE_CODEC_PNG_V1)
        .u16(2)
        .u32(64)
        .u32(128)
        .u64(1n)
        .bytes(encodeExtensions())
        .finish(),
    );

    await expect(opening).rejects.toThrow(/completed after client disposal/);
    const cleanup = lastRequest(transport);
    expect(cleanup).toMatchObject({
      family: YAS_FAMILY_SURFACE,
      kind: YAS_SURFACE_CLOSE_VIEW,
    });
    pushResult(transport, cleanup, new Uint8Array());
    await expect(
      surface.openView({
        surfaceHandle: 1n,
        width: 640,
        height: 480,
        maxFps: 60,
        decoderCapacity: 2,
        codecVersions: [YAS_SURFACE_CODEC_PNG_V1],
      }),
    ).rejects.toThrow(/client is disposed/);
  });

  it("closes peer views after any local Terminal or Surface admission failure", async () => {
    const terminalConnection = await connected();
    const terminals = new YasTerminalClient(terminalConnection.connection);
    const terminalBudget = terminalConnection.connection.receiveBudget.reserve(
      16n * 1024n * 1024n,
      16n * 1024n * 1024n,
    );
    const terminalOpening = terminals.openView({
      terminalHandle: 1n,
      rows: 24,
      cols: 80,
      maxFps: 60,
      codecVersions: [YAS_TERMINAL_GRID_CODEC_V1],
    });
    const terminalOpenRequest = lastRequest(terminalConnection.transport);
    pushResult(
      terminalConnection.transport,
      terminalOpenRequest,
      new YasWriter()
        .u32(17)
        .u16(YAS_TERMINAL_GRID_CODEC_V1)
        .u8(1)
        .u8(0)
        .u32(64)
        .u32(128)
        .u32(1)
        .bytes(encodeExtensions())
        .finish(),
    );
    await expect(terminalOpening).rejects.toThrow(/receive budget exhausted/);
    const terminalCleanup = lastRequest(terminalConnection.transport);
    expect(terminalCleanup).toMatchObject({
      family: YAS_FAMILY_TERMINAL,
      kind: YAS_TERMINAL_CLOSE_VIEW,
    });
    pushResult(terminalConnection.transport, terminalCleanup, new Uint8Array());
    terminalBudget.release();

    const terminalCodecOpening = terminals.openView({
      terminalHandle: 1n,
      rows: 24,
      cols: 80,
      maxFps: 60,
      codecVersions: [YAS_TERMINAL_GRID_CODEC_V1],
    });
    const terminalCodecRequest = lastRequest(terminalConnection.transport);
    pushResult(
      terminalConnection.transport,
      terminalCodecRequest,
      new YasWriter()
        .u32(18)
        .u16(YAS_TERMINAL_GRID_CODEC_V1 + 1)
        .u8(1)
        .u8(0)
        .u32(64)
        .u32(128)
        .u32(1)
        .bytes(encodeExtensions())
        .finish(),
    );
    await expect(terminalCodecOpening).rejects.toThrow(
      /unoffered Terminal codec/,
    );
    const terminalCodecCleanup = lastRequest(terminalConnection.transport);
    expect(terminalCodecCleanup).toMatchObject({
      family: YAS_FAMILY_TERMINAL,
      kind: YAS_TERMINAL_CLOSE_VIEW,
    });
    pushResult(
      terminalConnection.transport,
      terminalCodecCleanup,
      new Uint8Array(),
    );
    terminals.dispose();

    const surfaceTransport = new YasMockTransport();
    const surfaceConnection = new YasConnection(surfaceTransport, {
      ...connectionOptions,
      families: [
        { family: YAS_FAMILY_TRANSFER, versions: [1], required: true },
        { family: YAS_FAMILY_SURFACE, versions: [1], required: true },
      ],
    });
    const ready = surfaceConnection.connect();
    pushResult(
      surfaceTransport,
      lastRequest(surfaceTransport),
      serverHello([
        defaultFamilyDescriptors()[0]!,
        defaultFamilyDescriptors()[1]!,
        familyDescriptor(
          YAS_FAMILY_SURFACE,
          [
            [1, YAS_CLASS_REQUEST, YAS_SURFACE_OPEN_VIEW],
            [1, YAS_CLASS_REQUEST, YAS_SURFACE_CLOSE_VIEW],
          ],
          0,
          canonicalFamilyLimits(YAS_FAMILY_SURFACE),
        ),
      ]),
    );
    await ready;
    const surfaces = new YasSurfaceClient(surfaceConnection);
    const surfaceBudget = surfaceConnection.receiveBudget.reserve(
      16n * 1024n * 1024n,
      16n * 1024n * 1024n,
    );
    const surfaceOpening = surfaces.openView({
      surfaceHandle: 1n,
      width: 640,
      height: 480,
      maxFps: 60,
      decoderCapacity: 1,
      codecVersions: [YAS_SURFACE_CODEC_PNG_V1],
    });
    const surfaceOpenRequest = lastRequest(surfaceTransport);
    const openPayload = new YasCursor(surfaceOpenRequest.payload);
    openPayload.u64("Surface handle");
    openPayload.u32("Surface width");
    openPayload.u32("Surface height");
    openPayload.u16("Surface maximum FPS");
    expect(openPayload.u8("Surface decoder capacity")).toBe(1);
    pushResult(
      surfaceTransport,
      surfaceOpenRequest,
      new YasWriter()
        .u32(19)
        .u16(YAS_SURFACE_CODEC_PNG_V1)
        .u16(1)
        .u32(64)
        .u32(128)
        .u64(1n)
        .bytes(encodeExtensions())
        .finish(),
    );
    await expect(surfaceOpening).rejects.toThrow(/receive budget exhausted/);
    const surfaceCleanup = lastRequest(surfaceTransport);
    expect(surfaceCleanup).toMatchObject({
      family: YAS_FAMILY_SURFACE,
      kind: YAS_SURFACE_CLOSE_VIEW,
    });
    pushResult(surfaceTransport, surfaceCleanup, new Uint8Array());
    surfaceBudget.release();

    const surfaceCodecOpening = surfaces.openView({
      surfaceHandle: 1n,
      width: 640,
      height: 480,
      maxFps: 60,
      decoderCapacity: 1,
      codecVersions: [YAS_SURFACE_CODEC_PNG_V1],
    });
    const surfaceCodecRequest = lastRequest(surfaceTransport);
    pushResult(
      surfaceTransport,
      surfaceCodecRequest,
      new YasWriter()
        .u32(20)
        .u16(YAS_SURFACE_CODEC_H264_V1)
        .u16(1)
        .u32(64)
        .u32(128)
        .u64(1n)
        .bytes(encodeExtensions())
        .finish(),
    );
    await expect(surfaceCodecOpening).rejects.toThrow(
      /unoffered Surface codec/,
    );
    const surfaceCodecCleanup = lastRequest(surfaceTransport);
    expect(surfaceCodecCleanup).toMatchObject({
      family: YAS_FAMILY_SURFACE,
      kind: YAS_SURFACE_CLOSE_VIEW,
    });
    pushResult(surfaceTransport, surfaceCodecCleanup, new Uint8Array());
    surfaces.dispose();
  });

  it("enforces Core negotiation and catalogue replacement invariants", async () => {
    expect(() =>
      encodeClientHello({
        ...connectionOptions,
        receiveMaxDatagram: 1,
      }),
    ).toThrow(/datagram/);
    expect(() =>
      encodeClientHello({
        ...connectionOptions,
        receiveMaxBuffered: 0n,
      }),
    ).toThrow(/buffered/);
    expect(() =>
      encodeClientHello({
        ...connectionOptions,
        codecs: [0],
      }),
    ).toThrow(/codec/);
    expect(() =>
      encodeClientHello({
        ...connectionOptions,
        families: [{ family: YAS_FAMILY_RELAY, versions: [0] }],
      }),
    ).toThrow(/version/);
    expect(() => decodeCancel(new Uint8Array(4))).toThrow(/zero/);
    expect(() =>
      decodeFamilyUpdate(
        new YasWriter()
          .u64(2n)
          .bytes(
            familyDescriptor(YAS_FAMILY_RELAY, [
              [1, YAS_CLASS_RESULT, YAS_RELAY_CONNECT],
            ]),
          )
          .finish(),
      ),
    ).toThrow(/class/);
    expect(() =>
      decodeFamilyUpdate(
        new YasWriter()
          .u64(2n)
          .bytes(
            familyDescriptor(YAS_FAMILY_RELAY, [
              [2, YAS_CLASS_REQUEST, YAS_RELAY_CONNECT],
            ]),
          )
          .finish(),
      ),
    ).toThrow(/forbidden direction/);

    const terminalLimits = canonicalFamilyLimits(YAS_FAMILY_TERMINAL);
    const decodeTerminalLimits = (
      limits: readonly import("../yas").YasExtension[],
    ) =>
      decodeFamilyUpdate(
        new YasWriter()
          .u64(2n)
          .bytes(familyDescriptor(YAS_FAMILY_TERMINAL, [], 0, limits))
          .finish(),
      );
    expect(() => decodeTerminalLimits(terminalLimits.slice(1))).toThrow(
      /missing required family limit 1/,
    );
    expect(() =>
      decodeTerminalLimits([
        { tag: 1, value: new Uint8Array(8) },
        ...terminalLimits.slice(1),
      ]),
    ).toThrow(/family limit 1 must be 4 bytes/);
    expect(() =>
      decodeTerminalLimits([
        { tag: 1, value: new YasWriter().u32(0).finish() },
        ...terminalLimits.slice(1),
      ]),
    ).toThrow(/family limit 1 is outside its canonical bounds/);
    expect(() =>
      decodeTerminalLimits([
        { tag: 1, value: new YasWriter().u32(65_536).finish() },
        ...terminalLimits.slice(1),
      ]),
    ).toThrow(/family limit 1 is outside its canonical bounds/);
    expect(() =>
      decodeTerminalLimits([
        ...terminalLimits,
        { tag: 0xffff, value: new Uint8Array() },
      ]),
    ).not.toThrow();

    const kvLimits = canonicalFamilyLimits(YAS_FAMILY_KV).map((limit) =>
      limit.tag === YAS_KV_LIMIT_MAX_VALUE_BYTES
        ? { ...limit, value: new YasWriter().u64(1n).finish() }
        : limit.tag === YAS_KV_LIMIT_MAX_INLINE_BYTES
          ? { ...limit, value: new YasWriter().u32(2).finish() }
          : limit,
    );
    expect(() =>
      decodeFamilyUpdate(
        new YasWriter()
          .u64(2n)
          .bytes(familyDescriptor(YAS_FAMILY_KV, [], 0, kvLimits))
          .finish(),
      ),
    ).toThrow(/KV inline byte limit exceeds/);

    const eventsLimits = canonicalFamilyLimits(YAS_FAMILY_EVENTS).map(
      (limit) =>
        limit.tag === YAS_EVENTS_LIMIT_MIN_RING_BYTES
          ? { ...limit, value: new YasWriter().u64(8192n).finish() }
          : limit.tag === YAS_EVENTS_LIMIT_MAX_RING_BYTES
            ? { ...limit, value: new YasWriter().u64(4096n).finish() }
            : limit,
    );
    expect(() =>
      decodeFamilyUpdate(
        new YasWriter()
          .u64(2n)
          .bytes(familyDescriptor(YAS_FAMILY_EVENTS, [], 0, eventsLimits))
          .finish(),
      ),
    ).toThrow(/Events minimum ring byte limit exceeds/);

    const relayLimits = canonicalFamilyLimits(YAS_FAMILY_RELAY).map((limit) =>
      limit.tag === YAS_RELAY_LIMIT_MAX_LINKS_PER_SESSION
        ? { ...limit, value: new YasWriter().u32(1).finish() }
        : limit.tag === YAS_RELAY_LIMIT_MAX_PENDING_CONNECTS
          ? { ...limit, value: new YasWriter().u32(2).finish() }
          : limit,
    );
    expect(() =>
      decodeFamilyUpdate(
        new YasWriter()
          .u64(2n)
          .bytes(familyDescriptor(YAS_FAMILY_RELAY, [], 0, relayLimits))
          .finish(),
      ),
    ).toThrow(/Relay pending-connect limit exceeds/);

    const shrinking = await connected();
    pushEvent(
      shrinking.transport,
      YAS_FAMILY_CORE,
      YAS_CORE_SESSION_UPDATE,
      new YasWriter()
        .u64(2n)
        .u32(512 * 1024)
        .u32(4 * 1024 * 1024)
        .u32(0)
        .u64(16n * 1024n * 1024n)
        .bytes(encodeExtensions())
        .finish(),
    );
    expect(shrinking.transport.status).toBe("closed");
    expect(shrinking.connection.ready).toBe(false);

    const unknownFamily = await connected();
    pushEvent(
      unknownFamily.transport,
      YAS_FAMILY_CORE,
      YAS_CORE_FAMILY_UPDATE,
      new YasWriter().u64(2n).bytes(familyDescriptor(0x7777)).finish(),
    );
    expect(unknownFamily.transport.status).toBe("closed");

    const malformedGoAway = await connected();
    pushEvent(
      malformedGoAway.transport,
      YAS_FAMILY_CORE,
      YAS_CORE_GOAWAY,
      new YasWriter().u16(0).u16(0).u64(0n).u32(1).u8(0).finish(),
    );
    expect(malformedGoAway.transport.status).toBe("closed");
  });

  it("rejects selected families whose canonical dependencies were omitted", async () => {
    const transferDependents = [
      YAS_FAMILY_RELAY,
      YAS_FAMILY_TERMINAL,
      YAS_FAMILY_SURFACE,
      YAS_FAMILY_SELECTION,
      YAS_FAMILY_DESKTOP,
      YAS_FAMILY_MEDIA,
      YAS_FAMILY_FONT,
      YAS_FAMILY_FS,
      YAS_FAMILY_GIT,
      YAS_FAMILY_LSP,
      YAS_FAMILY_KV,
      YAS_FAMILY_PROCESS,
      YAS_FAMILY_NET,
      YAS_FAMILY_CHANNEL,
      YAS_FAMILY_EXTENSION,
      YAS_FAMILY_EVENTS,
      YAS_FAMILY_ENV,
    ];
    for (const family of transferDependents) {
      const transport = new YasMockTransport();
      const connection = new YasConnection(transport, {
        ...connectionOptions,
        families: [{ family, versions: [1] }],
      });
      const ready = connection.connect();
      pushResult(
        transport,
        lastRequest(transport),
        serverHello([defaultFamilyDescriptors()[0]!, familyDescriptor(family)]),
      );
      await expect(ready).rejects.toThrow(/without dependency 0x1/);
      expect(transport.status).toBe("closed");
    }

    const noChannel = new YasMockTransport();
    const extensionConnection = new YasConnection(noChannel, {
      ...connectionOptions,
      families: [
        { family: YAS_FAMILY_TRANSFER, versions: [1] },
        { family: YAS_FAMILY_EXTENSION, versions: [1] },
      ],
    });
    const extensionReady = extensionConnection.connect();
    pushResult(
      noChannel,
      lastRequest(noChannel),
      serverHello([
        defaultFamilyDescriptors()[0]!,
        defaultFamilyDescriptors()[1]!,
        familyDescriptor(YAS_FAMILY_EXTENSION),
      ]),
    );
    await expect(extensionReady).rejects.toThrow(/without dependency 0x42/);

    const clientOnly = new YasMockTransport();
    const clientConnection = new YasConnection(clientOnly, {
      ...connectionOptions,
      families: [{ family: YAS_FAMILY_CLIENT, versions: [1] }],
    });
    const clientReady = clientConnection.connect();
    pushResult(
      clientOnly,
      lastRequest(clientOnly),
      serverHello([
        defaultFamilyDescriptors()[0]!,
        familyDescriptor(YAS_FAMILY_CLIENT),
      ]),
    );
    await expect(clientReady).resolves.toBeDefined();
  });

  it("negotiates exact family versions and correlates out-of-order Results", async () => {
    const { transport, connection } = await connected();
    expect(connection.family(YAS_FAMILY_RELAY).version).toBe(1);
    const first = connection.request(YAS_FAMILY_CORE, 1, new Uint8Array(8));
    const firstFrame = lastRequest(transport);
    const second = connection.request(YAS_FAMILY_CORE, 1, new Uint8Array(8));
    const secondFrame = lastRequest(transport);
    pushResult(transport, secondFrame, new Uint8Array([2]));
    pushResult(transport, firstFrame, new Uint8Array([1]));
    await expect(first).resolves.toEqual(new Uint8Array([1]));
    await expect(second).resolves.toEqual(new Uint8Array([2]));
  });

  it("answers peer Core PING Requests natively", async () => {
    const { transport } = await connected();
    transport.push(
      encodeYasFrame({
        family: YAS_FAMILY_CORE,
        kind: YAS_CORE_PING,
        class: YAS_CLASS_REQUEST,
        requestId: 41,
        payload: new YasWriter().u64(123n).finish(),
      }),
    );
    const frame = decodeYasFrame(transport.sent.at(-1)!);
    expect(frame).toMatchObject({
      family: YAS_FAMILY_CORE,
      kind: YAS_CORE_PING,
      class: YAS_CLASS_RESULT,
      requestId: 41,
    });
    const result = decodeResultPayload(frame.payload);
    expect(result.status).toBe(YAS_STATUS_OK);
    const timing = decodePingResult(result.body);
    expect(timing.receiverSendNs).toBeGreaterThanOrEqual(
      timing.receiverReceiveNs,
    );

    transport.push(
      encodeYasFrame({
        family: YAS_FAMILY_CORE,
        kind: YAS_CORE_PING,
        class: YAS_CLASS_REQUEST,
        requestId: 42,
        payload: new Uint8Array(7),
      }),
    );
    const malformed = decodeYasFrame(transport.sent.at(-1)!);
    expect(decodeResultPayload(malformed.payload).status).toBe(
      YAS_STATUS_INVALID,
    );
    expect(transport.status).toBe("connected");
  });

  it("cancels admitted peer Requests and rejects active Request ID reuse", async () => {
    const cancelled = await connectedWithSelection();
    let observedSignal: AbortSignal | undefined;
    cancelled.connection.handleRequests(
      YAS_FAMILY_SELECTION,
      YAS_SELECTION_GET,
      ({ signal }) => {
        observedSignal = signal;
        return new Promise<Uint8Array>(() => {});
      },
    );
    cancelled.transport.push(
      encodeYasFrame({
        family: YAS_FAMILY_SELECTION,
        kind: YAS_SELECTION_GET,
        class: YAS_CLASS_REQUEST,
        requestId: 51,
        sensitive: true,
        payload: new Uint8Array(),
      }),
    );
    cancelled.transport.push(
      encodeYasFrame({
        family: YAS_FAMILY_CORE,
        kind: YAS_CORE_CANCEL,
        class: YAS_CLASS_REQUEST,
        requestId: 53,
        payload: new YasWriter().u32(51).finish(),
      }),
    );
    await vi.waitFor(() => {
      const results = cancelled.transport.sent
        .slice(2)
        .map((bytes) => decodeYasFrame(bytes))
        .filter((frame) => frame.class === YAS_CLASS_RESULT);
      expect(results).toHaveLength(2);
      const cancel = results.find((frame) => frame.requestId === 53)!;
      const target = results.find((frame) => frame.requestId === 51)!;
      expect(decodeResultPayload(cancel.payload).status).toBe(YAS_STATUS_OK);
      expect(decodeResultPayload(target.payload).status).toBe(
        YAS_STATUS_CANCELLED,
      );
    });
    expect(observedSignal?.aborted).toBe(true);

    const duplicate = await connectedWithSelection();
    duplicate.connection.handleRequests(
      YAS_FAMILY_SELECTION,
      YAS_SELECTION_GET,
      () => new Promise<Uint8Array>(() => {}),
    );
    const request = encodeYasFrame({
      family: YAS_FAMILY_SELECTION,
      kind: YAS_SELECTION_GET,
      class: YAS_CLASS_REQUEST,
      requestId: 61,
      sensitive: true,
      payload: new Uint8Array(),
    });
    duplicate.transport.push(request);
    duplicate.transport.push(request);
    expect(duplicate.transport.status).toBe("closed");
  });

  it("bounds pipelined peer Requests by retained count", async () => {
    const hostile = await connectedWithSelection();
    const handler = vi.fn(() => new Promise<Uint8Array>(() => {}));
    hostile.connection.handleRequests(
      YAS_FAMILY_SELECTION,
      YAS_SELECTION_GET,
      handler,
    );
    for (
      let requestId = 1;
      requestId <= YAS_MAX_RETAINED_INCOMING_REQUESTS;
      requestId++
    ) {
      hostile.transport.push(
        encodeYasFrame({
          family: YAS_FAMILY_SELECTION,
          kind: YAS_SELECTION_GET,
          class: YAS_CLASS_REQUEST,
          requestId: 1000 + requestId,
          sensitive: true,
          payload: new Uint8Array(),
        }),
      );
    }
    expect(hostile.transport.status).toBe("connected");
    hostile.transport.push(
      encodeYasFrame({
        family: YAS_FAMILY_SELECTION,
        kind: YAS_SELECTION_GET,
        class: YAS_CLASS_REQUEST,
        requestId: 2000,
        sensitive: true,
        payload: new Uint8Array(),
      }),
    );
    expect(handler).toHaveBeenCalledTimes(YAS_MAX_RETAINED_INCOMING_REQUESTS);
    expect(hostile.transport.status).toBe("closed");
  });

  it("shares the receive byte budget across retained peer Request payloads", async () => {
    const bounded = await connectedWithSelection({ receiveMaxBuffered: 8n });
    const handler = vi.fn(() => new Promise<Uint8Array>(() => {}));
    bounded.connection.handleRequests(
      YAS_FAMILY_SELECTION,
      YAS_SELECTION_GET,
      handler,
    );
    for (const [requestId, value] of [
      [71, 1],
      [72, 2],
    ] as const) {
      bounded.transport.push(
        encodeYasFrame({
          family: YAS_FAMILY_SELECTION,
          kind: YAS_SELECTION_GET,
          class: YAS_CLASS_REQUEST,
          requestId,
          sensitive: true,
          payload: new Uint8Array(5).fill(value),
        }),
      );
    }
    // The first payload is retained and reaches the handler. The second does
    // not fit the remaining budget, so it is refused rather than admitted --
    // and a full budget is backpressure, not a protocol violation, so the
    // session survives it.
    expect(handler).toHaveBeenCalledTimes(1);
    const refusal = decodeYasFrame(bounded.transport.sent.at(-1)!);
    expect(refusal).toMatchObject({
      family: YAS_FAMILY_SELECTION,
      kind: YAS_SELECTION_GET,
      class: YAS_CLASS_RESULT,
      requestId: 72,
    });
    expect(decodeResultPayload(refusal.payload).status).toBe(
      YAS_STATUS_RESOURCE_EXHAUSTED,
    );
    expect(bounded.transport.status).toBe("connected");
  });

  it("releases retained peer Request bytes after the Result is sent", async () => {
    const bounded = await connectedWithSelection({ receiveMaxBuffered: 8n });
    bounded.connection.handleRequests(
      YAS_FAMILY_SELECTION,
      YAS_SELECTION_GET,
      async () => new Uint8Array(),
    );
    const send = (requestId: number) =>
      bounded.transport.push(
        encodeYasFrame({
          family: YAS_FAMILY_SELECTION,
          kind: YAS_SELECTION_GET,
          class: YAS_CLASS_REQUEST,
          requestId,
          sensitive: true,
          payload: new Uint8Array(8),
        }),
      );
    send(81);
    await vi.waitFor(() =>
      expect(
        bounded.transport.sent
          .slice(2)
          .map((bytes) => decodeYasFrame(bytes))
          .some(
            (frame) =>
              frame.class === YAS_CLASS_RESULT && frame.requestId === 81,
          ),
      ).toBe(true),
    );
    send(82);
    await vi.waitFor(() =>
      expect(
        bounded.transport.sent
          .slice(2)
          .map((bytes) => decodeYasFrame(bytes))
          .some(
            (frame) =>
              frame.class === YAS_CLASS_RESULT && frame.requestId === 82,
          ),
      ).toBe(true),
    );
    expect(bounded.transport.status).toBe("connected");
  });

  it("can preserve a non-OK Result for family APIs that expose statuses", async () => {
    const { transport, connection } = await connected();
    const result = connection.requestResult(
      YAS_FAMILY_CORE,
      YAS_CORE_PING,
      new Uint8Array(8),
    );
    const request = lastRequest(transport);
    const detail = new YasWriter()
      .u16(1)
      .u16(0)
      .u32(2)
      .bytes(new Uint8Array([3, 4]))
      .finish();
    transport.push(
      encodeYasFrame({
        family: request.family,
        kind: request.kind,
        class: YAS_CLASS_RESULT,
        requestId: request.requestId,
        payload: encodeResultPayload(
          YAS_STATUS_UNAVAILABLE,
          new Uint8Array(),
          detail,
        ),
      }),
    );
    await expect(result).resolves.toEqual({
      status: YAS_STATUS_UNAVAILABLE,
      detail,
      body: new Uint8Array(),
    });

    const throwing = connection.request(
      YAS_FAMILY_CORE,
      YAS_CORE_PING,
      new Uint8Array(8),
    );
    const throwingRequest = lastRequest(transport);
    transport.push(
      encodeYasFrame({
        family: throwingRequest.family,
        kind: throwingRequest.kind,
        class: YAS_CLASS_RESULT,
        requestId: throwingRequest.requestId,
        payload: encodeResultPayload(YAS_STATUS_UNAVAILABLE),
      }),
    );
    await expect(throwing).rejects.toMatchObject({
      status: YAS_STATUS_UNAVAILABLE,
    });
  });

  it("keys generated frame policies by class and rejects unadvertised sends", async () => {
    const request = decodeYasFrame(
      encodeYasFrame({
        family: YAS_FAMILY_SELECTION,
        kind: YAS_SELECTION_SET,
        class: YAS_CLASS_REQUEST,
        requestId: 7,
        payload: new Uint8Array(0),
      }),
    );
    expect(request.sensitive).toBe(true);
    expect(() =>
      encodeYasFrame({
        family: YAS_FAMILY_SELECTION,
        kind: YAS_SELECTION_SET,
        class: YAS_CLASS_REQUEST,
        requestId: 7,
        sensitive: false,
        payload: new Uint8Array(0),
      }),
    ).toThrow(/SENSITIVE/);
    expect(
      decodeYasFrame(
        encodeYasFrame({
          family: YAS_FAMILY_SELECTION,
          kind: YAS_SELECTION_SET,
          class: YAS_CLASS_EVENT,
          sensitive: false,
          payload: new Uint8Array(0),
        }),
      ).sensitive,
    ).toBe(false);
    expect(() =>
      encodeYasFrame({
        family: YAS_FAMILY_TERMINAL,
        kind: YAS_TERMINAL_FRAME,
        class: YAS_CLASS_EVENT,
        compressed: true,
        payload: new Uint8Array(0),
      }),
    ).toThrow(/forbidden/);

    const { transport, connection } = await connected();
    const sent = transport.sent.length;
    await expect(
      connection.request(YAS_FAMILY_RELAY, 0x7fff),
    ).rejects.toMatchObject({ status: 2 });
    expect(transport.sent).toHaveLength(sent);
    expect(() => connection.sendEvent(YAS_FAMILY_RELAY, 0x7fff)).toThrow(
      /not advertised/,
    );
    pushEvent(transport, YAS_FAMILY_RELAY, 0x7fff, new Uint8Array(0));
    expect(transport.status).toBe("closed");
    expect(connection.ready).toBe(false);
  });

  it("applies contiguous catalogue updates, invalidates families, and reconnects after GOAWAY", async () => {
    const { transport, connection } = await connected({
      receiveMaxBuffered: 32n * 1024n * 1024n,
    });
    const invalidated: number[] = [];
    const catalogChanges: Array<{
      revision: bigint;
      families: readonly number[];
    }> = [];
    connection.onInvalidation(({ family }) => {
      if (family !== undefined) invalidated.push(family);
    });
    connection.onCatalogChange((change) => catalogChanges.push(change));
    const retained = connection.receiveBudget.reserve(20n * 1024n * 1024n);
    pushEvent(
      transport,
      YAS_FAMILY_CORE,
      YAS_CORE_SESSION_UPDATE,
      new YasWriter()
        .u64(2n)
        .u32(2 * 1024 * 1024)
        .u32(8 * 1024 * 1024)
        .u32(0)
        .u64(8n * 1024n * 1024n)
        .bytes(encodeExtensions())
        .finish(),
    );
    expect(connection.hello).toMatchObject({
      catalogRevision: 2n,
      receiveMaxFrame: 2 * 1024 * 1024,
      receiveMaxDecoded: 8 * 1024 * 1024,
      receiveMaxBuffered: 8n * 1024n * 1024n,
    });
    const remainder = connection.receiveBudget.reserve(12n * 1024n * 1024n);
    expect(() => connection.receiveBudget.reserve(1n)).toThrow(/exhausted/);
    remainder.release();
    retained.release();
    pushEvent(
      transport,
      YAS_FAMILY_CORE,
      YAS_CORE_FAMILY_UPDATE,
      new YasWriter()
        .u64(3n)
        .bytes(familyDescriptor(YAS_FAMILY_RELAY, [], 2))
        .finish(),
    );
    expect(invalidated).toContain(YAS_FAMILY_RELAY);
    expect(catalogChanges).toEqual([
      { revision: 3n, families: [YAS_FAMILY_RELAY] },
    ]);
    expect(() => connection.family(YAS_FAMILY_RELAY)).toThrow();
    pushEvent(
      transport,
      YAS_FAMILY_CORE,
      YAS_CORE_GOAWAY,
      new YasWriter().u16(0).u16(0).u64(0n).u32(0).finish(),
    );
    expect(connection.ready).toBe(false);
    expect(connection.goAway).toMatchObject({
      status: 0,
      closeDeadlineServerNs: 0n,
    });

    transport.setStatus("disconnected");
    transport.setStatus("connected");
    const reconnecting = connection.connect();
    const helloRequest = lastRequest(transport);
    expect(helloRequest.kind).toBe(YAS_CORE_HELLO);
    pushResult(transport, helloRequest, serverHello());
    await reconnecting;
    expect(connection.ready).toBe(true);
    expect(connection.family(YAS_FAMILY_RELAY).version).toBe(1);
    expect(connection.goAway).toBeNull();
  });

  it("resets negotiation before isolated invalidation callbacks and reconnects", async () => {
    const { transport, connection } = await connected();
    const cleanupError = new Error("cleanup failed");
    const reportError = vi.fn();
    const laterListener = vi.fn();
    const readyHellos = vi.fn();
    let reconnecting!: ReturnType<YasConnection["connect"]>;
    let reconnected = false;
    vi.stubGlobal("reportError", reportError);

    try {
      connection.onReady(readyHellos);
      expect(readyHellos).toHaveBeenCalledOnce();
      const sentBeforeAuthentication = transport.sent.length;
      transport.setStatus("authenticating");
      expect(connection.ready).toBe(false);
      await expect(connection.ping(1n)).rejects.toThrow(/not ready/);
      expect(transport.sent).toHaveLength(sentBeforeAuthentication);
      transport.setStatus("connected");
      expect(connection.ready).toBe(true);

      connection.onInvalidation(() => {
        expect(connection.ready).toBe(false);
        expect(connection.hello).toBeNull();
        expect(connection.families.size).toBe(0);
        reconnecting = connection.connect();
        void reconnecting.then(() => {
          reconnected = true;
        });
        throw cleanupError;
      });
      connection.onInvalidation(laterListener);

      expect(() => transport.setStatus("disconnected")).not.toThrow();
      expect(reportError).toHaveBeenCalledWith(cleanupError);
      expect(laterListener).toHaveBeenCalledOnce();
      expect(connection.ready).toBe(false);
      await Promise.resolve();
      expect(reconnected).toBe(false);

      transport.setStatus("authenticating");
      expect(connection.ready).toBe(false);
      transport.setStatus("connected");
      const helloRequest = lastRequest(transport);
      expect(helloRequest.kind).toBe(YAS_CORE_HELLO);
      pushResult(transport, helloRequest, serverHello());
      await reconnecting;
      expect(connection.ready).toBe(true);
      expect(readyHellos).toHaveBeenCalledTimes(2);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("clears dead-session state before synchronous request abort callbacks", async () => {
    const { transport, connection } = await connectedWithSelection();
    let abortObserved = false;
    connection.handleRequests(
      YAS_FAMILY_SELECTION,
      YAS_SELECTION_GET,
      ({ signal }) => {
        signal.addEventListener("abort", () => {
          abortObserved = true;
          expect(connection.ready).toBe(false);
          expect(connection.hello).toBeNull();
          expect(connection.families.size).toBe(0);
        });
        return new Promise<Uint8Array>(() => {});
      },
    );
    transport.push(
      encodeYasFrame({
        family: YAS_FAMILY_SELECTION,
        kind: YAS_SELECTION_GET,
        class: YAS_CLASS_REQUEST,
        requestId: 71,
        sensitive: true,
        payload: new Uint8Array(),
      }),
    );

    transport.setStatus("disconnected");

    expect(abortObserved).toBe(true);
  });

  it("preserves a synchronous send failure through transport close status", async () => {
    const { transport, connection } = await connected();
    const sendError = new Error("edge write failed");
    const invalidations: Error[] = [];
    connection.onInvalidation(({ error }) => invalidations.push(error));
    transport.send = () => {
      throw sendError;
    };

    const request = connection.request(
      YAS_FAMILY_CORE,
      YAS_CORE_PING,
      new Uint8Array(),
    );
    await expect(request).rejects.toBe(sendError);

    expect(transport.status).toBe("closed");
    expect(invalidations).toHaveLength(2);
    expect(invalidations[0]).toBeInstanceOf(YasProtocolError);
    expect(invalidations[0]?.message).toBe(sendError.message);
    expect(invalidations[1]).toBe(invalidations[0]);
  });

  it("propagates dependency loss and narrowing to dependent family clients", async () => {
    const unavailable = await connected();
    const unavailableInvalidations: number[] = [];
    unavailable.connection.onInvalidation(({ family }) => {
      if (family !== undefined) unavailableInvalidations.push(family);
    });
    pushEvent(
      unavailable.transport,
      YAS_FAMILY_CORE,
      YAS_CORE_FAMILY_UPDATE,
      new YasWriter()
        .u64(2n)
        .bytes(
          familyDescriptor(
            YAS_FAMILY_TRANSFER,
            [
              [3, YAS_CLASS_EVENT, YAS_TRANSFER_BYTE_DATA],
              [3, YAS_CLASS_EVENT, YAS_TRANSFER_MESSAGE_DATA],
              [3, YAS_CLASS_EVENT, YAS_TRANSFER_CREDIT],
              [3, YAS_CLASS_EVENT, YAS_TRANSFER_CLOSE],
              [3, YAS_CLASS_EVENT, YAS_TRANSFER_RESET],
            ],
            2,
          ),
        )
        .finish(),
    );
    expect(unavailable.transport.status).toBe("connected");
    expect(unavailableInvalidations).toEqual([
      YAS_FAMILY_TRANSFER,
      YAS_FAMILY_RELAY,
      YAS_FAMILY_TERMINAL,
      YAS_FAMILY_FONT,
    ]);

    const narrowed = await connected();
    const narrowedInvalidations: number[] = [];
    narrowed.connection.onInvalidation(({ family }) => {
      if (family !== undefined) narrowedInvalidations.push(family);
    });
    pushEvent(
      narrowed.transport,
      YAS_FAMILY_CORE,
      YAS_CORE_FAMILY_UPDATE,
      new YasWriter()
        .u64(2n)
        .bytes(
          familyDescriptor(YAS_FAMILY_TRANSFER, [
            [3, YAS_CLASS_EVENT, YAS_TRANSFER_BYTE_DATA],
            [3, YAS_CLASS_EVENT, YAS_TRANSFER_MESSAGE_DATA],
            [3, YAS_CLASS_EVENT, YAS_TRANSFER_CREDIT],
            [3, YAS_CLASS_EVENT, YAS_TRANSFER_CLOSE],
          ]),
        )
        .finish(),
    );
    expect(narrowed.transport.status).toBe("connected");
    expect(narrowedInvalidations).toEqual([
      YAS_FAMILY_TRANSFER,
      YAS_FAMILY_RELAY,
      YAS_FAMILY_TERMINAL,
      YAS_FAMILY_FONT,
    ]);
  });

  it("resolves SHUTDOWN before GOAWAY and drains admitted Results", async () => {
    const { transport, connection } = await connected();
    const operationId = new Uint8Array(16).fill(7);
    const shutdown = connection.shutdown(operationId, 50n, "maintenance");
    const shutdownRequest = lastRequest(transport);
    expect(shutdownRequest).toMatchObject({
      family: YAS_FAMILY_CORE,
      kind: YAS_CORE_SHUTDOWN,
      sensitive: true,
    });
    pushResult(transport, shutdownRequest, new Uint8Array());
    await shutdown;

    const admitted = connection.ping(1n);
    const admittedRequest = lastRequest(transport);
    pushEvent(
      transport,
      YAS_FAMILY_CORE,
      YAS_CORE_GOAWAY,
      new YasWriter().u16(0).u16(0).u64(999n).u32(0).finish(),
    );
    await expect(connection.ping(2n)).rejects.toThrow(/not ready/);
    pushResult(
      transport,
      admittedRequest,
      new YasWriter().u64(2n).u64(3n).finish(),
    );
    await expect(admitted).resolves.toEqual({
      receiverReceiveNs: 2n,
      receiverSendNs: 3n,
    });
  });

  it("resynchronizes a skipped catalogue revision with SESSION_INFO", async () => {
    const { transport, connection } = await connected({
      receiveMaxBuffered: 32n * 1024n * 1024n,
    });
    const invalidated: number[] = [];
    const catalogChanges: Array<{
      revision: bigint;
      families: readonly number[];
    }> = [];
    connection.onInvalidation(({ family }) => {
      if (family !== undefined) invalidated.push(family);
    });
    connection.onCatalogChange((change) => catalogChanges.push(change));
    const retained = connection.receiveBudget.reserve(28n * 1024n * 1024n);
    pushEvent(
      transport,
      YAS_FAMILY_CORE,
      YAS_CORE_SESSION_UPDATE,
      new YasWriter()
        .u64(3n)
        .u32(1024 * 1024)
        .u32(4 * 1024 * 1024)
        .u32(0)
        .u64(1024n)
        .bytes(encodeExtensions())
        .finish(),
    );
    const infoRequest = lastRequest(transport);
    expect(infoRequest.kind).toBe(YAS_CORE_SESSION_INFO);
    const descriptors = defaultFamilyDescriptors();
    descriptors[2] = familyDescriptor(YAS_FAMILY_RELAY, [], 2);
    pushResult(
      transport,
      infoRequest,
      new YasWriter()
        .bytes(new Uint8Array(16).fill(2))
        .u64(3n)
        .u32(2 * 1024 * 1024)
        .u32(8 * 1024 * 1024)
        .u32(0)
        .u64(4n * 1024n * 1024n)
        .u64(456n)
        .u16(descriptors.length)
        .bytes(concat(descriptors))
        .bytes(encodeExtensions())
        .finish(),
    );
    await vi.waitFor(() => expect(connection.hello?.catalogRevision).toBe(3n));
    expect(connection.hello?.receiveMaxFrame).toBe(2 * 1024 * 1024);
    expect(connection.hello?.receiveMaxBuffered).toBe(4n * 1024n * 1024n);
    const remainder = connection.receiveBudget.reserve(4n * 1024n * 1024n);
    expect(() => connection.receiveBudget.reserve(1n)).toThrow(/exhausted/);
    remainder.release();
    retained.release();
    expect(connection.families.has(YAS_FAMILY_FONT)).toBe(true);
    expect(invalidated).toContain(YAS_FAMILY_RELAY);
    expect(catalogChanges).toHaveLength(1);
    expect(catalogChanges[0]).toMatchObject({ revision: 3n });
    expect(catalogChanges[0]!.families).toContain(YAS_FAMILY_RELAY);
  });

  it("validates STATE marker records, family flags, and required record kinds", () => {
    const unknownOptional = encodeTypedRecord({
      kind: 99,
      flags: 0,
      body: new Uint8Array([1]),
    });
    const unknownRequired = encodeTypedRecord({
      kind: 99,
      flags: 1,
      body: new Uint8Array([1]),
    });
    expect(() =>
      decodeStateEvent(
        stateEvent(1, YAS_STATE_SNAPSHOT_BEGIN, 0n, 1n, [unknownOptional]),
      ),
    ).toThrow(/marker/);
    expect(
      decodeStateEvent(
        stateEvent(1, YAS_STATE_SNAPSHOT_END, 1n, 1n, [
          encodeTypedRecord({
            kind: YAS_STATE_ADD,
            flags: 0,
            body: new Uint8Array([1]),
          }),
        ]),
      ).batch.records,
    ).toHaveLength(1);
    expect(
      decodeStateEvent(
        new YasWriter()
          .u32(1)
          .u8(YAS_STATE_DELTA)
          .u8(4)
          .u16(0)
          .u64(1n)
          .u64(2n)
          .u16(1)
          .bytes(unknownOptional)
          .finish(),
        { allowedFlags: 4 },
      ).batch.records,
    ).toEqual([]);
    expect(() =>
      decodeStateEvent(
        stateEvent(1, YAS_STATE_DELTA, 1n, 2n, [unknownRequired]),
      ),
    ).toThrow(/unknown required/);
  });

  it("bounds aggregate Transfer credit, serializes messages, and rejects transfer ID zero", async () => {
    const firstConnection = await connected();
    const manager = transfersFor(firstConnection.connection);
    const descriptor = (
      transferId: number,
      receiverSendCredit: bigint,
    ): YasTransferDescriptor => ({
      transferId,
      mode: YAS_TRANSFER_MODE_BYTE,
      direction: YAS_TRANSFER_RECEIVER_TO_SENDER,
      flags: 0,
      receiverSendCredit,
      senderSendCredit: 0n,
      maxItemBytes: 0n,
      maxChunkBytes: 64 * 1024,
      contentFamily: YAS_FAMILY_RELAY,
      contentKind: 0,
      contentVersion: 1,
      extensions: [],
      maxOpenMessages: 1,
    });
    const lease1 = manager.reserveReceiveCredit(1024n);
    const first = manager.acceptServerDescriptor(
      descriptor(2, 9n * 1024n * 1024n),
      lease1,
    );
    const lease2 = manager.reserveReceiveCredit(1024n);
    expect(() =>
      manager.acceptServerDescriptor(descriptor(4, 9n * 1024n * 1024n), lease2),
    ).toThrow(/aggregate/);
    first.reset();

    const upload = manager.acceptServerUploadDescriptor(descriptor(6, 64n));
    expect(() =>
      manager.acceptServerUploadDescriptor({
        ...descriptor(8, 64n),
        direction:
          YAS_TRANSFER_RECEIVER_TO_SENDER | YAS_TRANSFER_SENDER_TO_RECEIVER,
      }),
    ).toThrow(/server-to-client/);
    // Upload-only Transfers consume no browser receive budget.
    const fullReceiveLease = manager.reserveReceiveCredit(
      16n * 1024n * 1024n,
      16n * 1024n * 1024n,
    );
    fullReceiveLease.release();
    upload.reset();

    const uploadStage = { stagingHandle: 99n, expiresServerNs: 1_000n };
    const uploadExtensions = [
      {
        tag: YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION,
        required: true,
        value: new Uint8Array(),
      },
      {
        tag: YAS_TRANSFER_UPLOAD_STAGE_EXTENSION,
        required: true,
        value: new YasWriter()
          .u64(uploadStage.stagingHandle)
          .u64(uploadStage.expiresServerNs)
          .finish(),
      },
    ];
    const staged = (transferId: number): YasTransferDescriptor => ({
      ...descriptor(transferId, 64n),
      extensions: uploadExtensions,
      sensitiveContent: true,
      uploadStage,
    });
    const stagedFirst = manager.acceptServerUploadDescriptor(staged(8));
    const stagedSibling = manager.acceptServerUploadDescriptor(staged(10));
    const crossFamily = manager.acceptServerUploadDescriptor({
      ...staged(12),
      contentFamily: YAS_FAMILY_KV,
    });
    stagedFirst.reset();
    await Promise.all([stagedFirst.closed, stagedSibling.closed]);
    expect(manager.get(8)).toBeUndefined();
    expect(manager.get(10)).toBeUndefined();
    expect(manager.get(12)).toBe(crossFamily);
    crossFamily.reset();
    await crossFamily.closed;

    const clientAllocated = manager.createClientDescriptor({
      mode: YAS_TRANSFER_MODE_BYTE,
      direction: YAS_TRANSFER_SENDER_TO_RECEIVER,
      receiverSendCredit: 0n,
      senderSendCredit: 64n,
      maxItemBytes: 0n,
      maxChunkBytes: 64,
      contentFamily: YAS_FAMILY_RELAY,
      contentKind: 0,
      contentVersion: 1,
      extensions: [],
    });
    expect(clientAllocated.descriptor.transferId & 1).toBe(1);
    await clientAllocated.transfer.write(new Uint8Array([1, 2, 3]));
    clientAllocated.transfer.closeWrite();
    const allocatedData = firstConnection.transport.sent
      .slice(2)
      .map((bytes) => decodeYasFrame(bytes))
      .find(
        (frame) =>
          frame.family === YAS_FAMILY_TRANSFER &&
          frame.kind === YAS_TRANSFER_BYTE_DATA &&
          new DataView(
            frame.payload.buffer,
            frame.payload.byteOffset,
            frame.payload.byteLength,
          ).getUint32(0, true) === clientAllocated.descriptor.transferId,
      );
    expect(allocatedData).toBeDefined();

    const secondConnection = await connected();
    const messages = transfersFor(secondConnection.connection);
    const messageLease = messages.reserveReceiveCredit(1n);
    const messageTransfer = messages.acceptServerDescriptor(
      {
        ...descriptor(2, 2n),
        mode: YAS_TRANSFER_MODE_MESSAGE,
        maxItemBytes: 16n,
        maxChunkBytes: 2,
      },
      messageLease,
    );
    const firstMessage = messageTransfer.sendMessage(new Uint8Array([1, 2, 3]));
    const secondMessage = messageTransfer.sendMessage(
      new Uint8Array([4, 5, 6]),
    );
    await Promise.resolve();
    await Promise.resolve();
    pushEvent(
      secondConnection.transport,
      YAS_FAMILY_TRANSFER,
      YAS_TRANSFER_CREDIT,
      new YasWriter().u32(2).u64(6n).finish(),
    );
    await firstMessage;
    await secondMessage;
    // Decode explicitly: every fragment of sequence zero precedes sequence one.
    const decodedSequences = secondConnection.transport.sent
      .slice(2)
      .map((bytes) => decodeYasFrame(bytes))
      .filter(
        (frame) =>
          frame.family === YAS_FAMILY_TRANSFER &&
          frame.kind === YAS_TRANSFER_MESSAGE_DATA,
      )
      .map((frame) => {
        const cursor = new YasCursor(frame.payload);
        cursor.u32("transfer ID");
        return cursor.u64("message sequence");
      });
    expect(decodedSequences).toEqual([0n, 0n, 1n, 1n]);

    const zeroConnection = await connected();
    transfersFor(zeroConnection.connection);
    pushEvent(
      zeroConnection.transport,
      YAS_FAMILY_TRANSFER,
      YAS_TRANSFER_CREDIT,
      new YasWriter().u32(0).u64(1n).finish(),
    );
    expect(zeroConnection.transport.status).toBe("closed");
  });

  it("bounds Relay tunnel queues and hostile Terminal frame chunks", async () => {
    const relayConnection = await connected();
    const manager = transfersFor(relayConnection.connection);
    const lease = manager.reserveReceiveCredit(1n);
    const transfer = manager.acceptServerDescriptor(
      {
        transferId: 2,
        mode: YAS_TRANSFER_MODE_BYTE,
        direction: YAS_TRANSFER_RECEIVER_TO_SENDER,
        flags: 0,
        receiverSendCredit: 1n,
        senderSendCredit: 0n,
        maxItemBytes: 0n,
        maxChunkBytes: 2,
        contentFamily: YAS_FAMILY_RELAY,
        contentKind: 0,
        contentVersion: 1,
        extensions: [],
        maxOpenMessages: 1,
      },
      lease,
    );
    const tunnel = new YasRelayTunnelTransport(transfer);
    tunnel.connect();
    expect(() => tunnel.send(new Uint8Array(9))).toThrow(/high-water/);
    expect(tunnel.status).toBe("error");

    const terminalConnection = await connected();
    const terminals = new YasTerminalClient(terminalConnection.connection);
    const opening = terminals.openView({
      terminalHandle: 1n,
      rows: 24,
      cols: 80,
      maxFps: 60,
      codecVersions: [YAS_TERMINAL_GRID_CODEC_V1],
    });
    const openRequest = lastRequest(terminalConnection.transport);
    pushResult(
      terminalConnection.transport,
      openRequest,
      new YasWriter()
        .u32(7)
        .u16(YAS_TERMINAL_GRID_CODEC_V1)
        .u8(2)
        .u8(0)
        .u32(64)
        .u32(128)
        .u32(1)
        .bytes(encodeExtensions())
        .finish(),
    );
    await opening;
    pushEvent(
      terminalConnection.transport,
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_FRAME_CHUNK,
      new YasWriter()
        .u32(7)
        .u32(1)
        .u16(0)
        .u16(2)
        .u32(10)
        .bytes(new Uint8Array(4))
        .finish(),
      true,
    );
    pushEvent(
      terminalConnection.transport,
      YAS_FAMILY_TERMINAL,
      YAS_TERMINAL_FRAME_CHUNK,
      new YasWriter()
        .u32(7)
        .u32(2)
        .u16(0)
        .u16(2)
        .u32(10)
        .bytes(new Uint8Array(4))
        .finish(),
      true,
    );
    expect(terminalConnection.transport.status).toBe("closed");
    const fullBudget = terminalConnection.connection.receiveBudget.reserve(
      16n * 1024n * 1024n,
      16n * 1024n * 1024n,
    );
    fullBudget.release();
  });

  it("rejects a Relay tunnel descriptor without sensitive-content policy", async () => {
    const { transport, connection } = await connected();
    const relay = new YasRelayClient(connection);
    const route: YasRelayRoute = {
      handle: 11n,
      generation: 3n,
      availability: 0,
      transportHint: 0,
      flags: 0,
      name: "work",
      label: "Work",
      description: "remote",
      extensions: [],
    };
    const connecting = relay.connect(route, {
      initialReceiveCredit: 64n * 1024n,
    });
    const connectRequest = lastRequest(transport);
    pushResult(
      transport,
      connectRequest,
      new YasWriter()
        .u64(99n)
        .u64(route.handle)
        .u64(route.generation)
        .bytes(
          transferDescriptor(
            2,
            YAS_TRANSFER_RECEIVER_TO_SENDER | YAS_TRANSFER_SENDER_TO_RECEIVER,
            64n * 1024n,
            64n * 1024n,
            YAS_FAMILY_RELAY,
            0,
          ),
        )
        .finish(),
    );
    await expect(connecting).rejects.toThrow(/nonsensitive tunnel/);
    expect(transport.status).toBe("closed");
  });

  it("requires the snapshot after STATE RESET to use the announced target", async () => {
    const { transport, connection } = await connected();
    const relay = new YasRelayClient(connection);
    const watching = relay.routes.watch();
    pushResult(
      transport,
      lastRequest(transport),
      new YasWriter()
        .u32(7)
        .u8(0)
        .bytes(new Uint8Array(3))
        .u64(5n)
        .bytes(encodeExtensions())
        .finish(),
    );
    pushEvent(
      transport,
      YAS_FAMILY_RELAY,
      YAS_RELAY_STATE,
      stateEvent(7, YAS_STATE_SNAPSHOT_BEGIN, 0n, 5n),
    );
    pushEvent(
      transport,
      YAS_FAMILY_RELAY,
      YAS_RELAY_STATE,
      stateEvent(7, YAS_STATE_SNAPSHOT_END, 5n, 5n),
    );
    await watching;

    pushEvent(
      transport,
      YAS_FAMILY_RELAY,
      YAS_RELAY_STATE,
      stateEvent(7, YAS_STATE_RESET, 5n, 9n),
    );
    expect(relay.routes.snapshot.revision).toBe(0n);
    pushEvent(
      transport,
      YAS_FAMILY_RELAY,
      YAS_RELAY_STATE,
      stateEvent(7, YAS_STATE_SNAPSHOT_BEGIN, 0n, 10n),
    );
    expect(transport.status).toBe("closed");
  });

  it("materializes Relay state and carries a nested raw YAS byte stream", async () => {
    const { transport, connection } = await connected();
    const relay = new YasRelayClient(connection);
    const watch = relay.routes.watch();
    const watchRequest = lastRequest(transport);
    pushResult(
      transport,
      watchRequest,
      new YasWriter()
        .u32(7)
        .u8(0)
        .bytes(new Uint8Array(3))
        .u64(5n)
        .bytes(encodeExtensions())
        .finish(),
    );
    const routeBody = new YasWriter()
      .u64(11n)
      .u64(3n)
      .u8(1)
      .u8(2)
      .u16(1)
      .utf8U16("work")
      .utf8U16("Work")
      .utf8U32("remote")
      .bytes(encodeExtensions())
      .finish();
    pushEvent(
      transport,
      YAS_FAMILY_RELAY,
      YAS_RELAY_STATE,
      stateEvent(7, YAS_STATE_SNAPSHOT_BEGIN, 0n, 5n),
    );
    pushEvent(
      transport,
      YAS_FAMILY_RELAY,
      YAS_RELAY_STATE,
      stateEvent(7, YAS_STATE_SNAPSHOT_RECORDS, 5n, 5n, [
        encodeTypedRecord({ kind: YAS_STATE_ADD, flags: 0, body: routeBody }),
      ]),
    );
    pushEvent(
      transport,
      YAS_FAMILY_RELAY,
      YAS_RELAY_STATE,
      stateEvent(7, YAS_STATE_SNAPSHOT_END, 5n, 5n),
    );
    await watch;
    expect(relay.routes.snapshot).toMatchObject({
      revision: 5n,
      routes: [{ handle: 11n, generation: 3n, name: "work" }],
    });

    const route = relay.routes.snapshot.routes[0]!;
    const connecting = relay.connect(route, {
      initialReceiveCredit: 64n * 1024n,
    });
    const connectRequest = lastRequest(transport);
    expect(connectRequest.kind).toBe(YAS_RELAY_CONNECT);
    pushResult(
      transport,
      connectRequest,
      new YasWriter()
        .u64(99n)
        .u64(route.handle)
        .u64(route.generation)
        .bytes(
          transferDescriptor(
            2,
            YAS_TRANSFER_RECEIVER_TO_SENDER | YAS_TRANSFER_SENDER_TO_RECEIVER,
            64n * 1024n,
            64n * 1024n,
            YAS_FAMILY_RELAY,
            0,
            true,
          ),
        )
        .finish(),
    );
    // DATA is legal immediately after the Result; descriptor registration must
    // happen synchronously rather than in a later Promise continuation.
    pushEvent(
      transport,
      YAS_FAMILY_TRANSFER,
      YAS_TRANSFER_BYTE_DATA,
      new YasWriter()
        .u32(2)
        .u64(0n)
        .bytes(new Uint8Array([1, 2, 3]))
        .finish(),
      true,
    );
    const link = await connecting;
    const chunks: Uint8Array[] = [];
    link.transport.addEventListener("message", (chunk) =>
      chunks.push(chunk instanceof Uint8Array ? chunk : new Uint8Array(chunk)),
    );
    link.transport.connect();
    await Promise.resolve();
    expect(chunks).toEqual([new Uint8Array([1, 2, 3])]);

    link.transport.send(new Uint8Array([4, 5]));
    await Promise.resolve();
    await Promise.resolve();
    const sentData = transport.sent
      .slice(1)
      .map((frame) => decodeYasFrame(frame))
      .reverse()
      .find(
        (frame) =>
          frame.family === YAS_FAMILY_TRANSFER &&
          frame.kind === YAS_TRANSFER_BYTE_DATA,
      );
    expect(sentData?.payload.slice(12)).toEqual(new Uint8Array([4, 5]));
  });

  it("describes and fetches fonts with Transfer credit and hash verification", async () => {
    const { transport, connection } = await connected();
    const hash = (bytes: Uint8Array) => {
      const output = new Uint8Array(32);
      output[0] = bytes.reduce((sum, value) => (sum + value) & 0xff, 0);
      return output;
    };
    const fonts = new YasFontClient(connection, hash);
    const faceBytes = new Uint8Array([10, 20, 30]);
    const contentHash = hash(faceBytes);
    const faceRecord = new YasWriter()
      .u64(21n)
      .bytes(contentHash)
      .u64(BigInt(faceBytes.length))
      .u8(YAS_FONT_FORMAT_TRUETYPE)
      .u8(YAS_FONT_STYLE_NORMAL)
      .u16(4)
      .u16(400)
      .u16(400)
      .u16(400)
      .u16(1000)
      .u16(1000)
      .u16(1000)
      .i16(0)
      .u16(1000)
      .i32(600)
      .i32(800)
      .i32(-200)
      .i32(0)
      .utf8U16("Regular")
      .utf8U16("Test-Regular")
      .bytes(encodeExtensions())
      .finish();
    const descriptionBytes = new YasWriter()
      .utf8U16("Test")
      .u16(1)
      .u32(faceRecord.length)
      .bytes(faceRecord)
      .bytes(encodeExtensions())
      .finish();
    const descriptionHash = hash(descriptionBytes);
    const describing = fonts.describe({ handle: 10n, generation: 2n });
    const describeRequest = lastRequest(transport);
    expect(describeRequest.kind).toBe(YAS_FONT_DESCRIBE);
    pushResult(
      transport,
      describeRequest,
      new YasWriter()
        .u64(10n)
        .u64(2n)
        .bytes(descriptionHash)
        .u8(0)
        .bytes(new Uint8Array(3))
        .bytesU32(descriptionBytes)
        .finish(),
    );
    const description = await describing;
    expect(description.family).toBe("Test");
    expect(description.faces[0]).toMatchObject({ handle: 21n, byteLength: 3n });

    const face = description.faces[0]!;
    const fetching = fonts.fetch(face, 64n * 1024n);
    const fetchRequest = lastRequest(transport);
    expect(fetchRequest.kind).toBe(YAS_FONT_FETCH);
    pushResult(
      transport,
      fetchRequest,
      new YasWriter()
        .u64(face.handle)
        .bytes(face.contentHash)
        .u64(face.byteLength)
        .u8(face.format)
        .bytes(new Uint8Array(3))
        .bytes(
          transferDescriptor(
            4,
            YAS_TRANSFER_SENDER_TO_RECEIVER,
            0n,
            64n * 1024n,
            YAS_FAMILY_FONT,
            1,
          ),
        )
        .finish(),
    );
    pushEvent(
      transport,
      YAS_FAMILY_TRANSFER,
      YAS_TRANSFER_BYTE_DATA,
      new YasWriter().u32(4).u64(0n).bytes(faceBytes).finish(),
    );
    pushEvent(
      transport,
      YAS_FAMILY_TRANSFER,
      YAS_TRANSFER_CLOSE,
      new YasWriter().u32(4).u64(3n).u16(0).u16(0).u32(0).finish(),
    );
    await expect(fetching).resolves.toMatchObject({
      faceHandle: 21n,
      bytes: faceBytes,
    });
  });
});

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join(
    "",
  );
}

function fromHex(value: string): Uint8Array {
  return Uint8Array.from(value.match(/../g) ?? [], (byte) =>
    Number.parseInt(byte, 16),
  );
}

function vector(name: string): string {
  const value = YAS_GOLDEN_VECTORS.vectors.find(
    (candidate) => candidate.name === name,
  );
  if (!value) throw new Error(`missing generated YAS vector ${name}`);
  return value.hex;
}

class EdgeWebSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;
  static instances: EdgeWebSocket[] = [];

  readonly sent: unknown[] = [];
  readyState = EdgeWebSocket.CONNECTING;
  binaryType = "blob";
  onopen: ((event: Event) => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  onclose: ((event: CloseEvent) => void) | null = null;

  constructor(
    readonly url: string,
    readonly protocols?: string | string[],
  ) {
    EdgeWebSocket.instances.push(this);
  }

  send(data: unknown): void {
    this.sent.push(data);
  }

  close(): void {
    this.readyState = EdgeWebSocket.CLOSED;
    this.onclose?.({} as CloseEvent);
  }
}

describe("YAS edge browser transport", () => {
  beforeEach(() => {
    EdgeWebSocket.instances = [];
    vi.stubGlobal("WebSocket", EdgeWebSocket);
  });

  afterEach(() => vi.unstubAllGlobals());

  it("selects yas.v1, authenticates in text, then forwards binary messages", () => {
    const transport = new YasEdgeWebSocketTransport(
      "wss://example.test/edge",
      "bearer",
      { reconnect: false },
    );
    transport.connect();
    const socket = EdgeWebSocket.instances[0]!;
    expect(socket.protocols).toBe("yas.v1");
    socket.readyState = EdgeWebSocket.OPEN;
    socket.onopen?.({} as Event);
    expect(socket.sent).toEqual(["bearer"]);
    socket.onmessage?.({ data: "ok" } as MessageEvent);
    expect(transport.status).toBe("connected");
    const hello = encodeYasFrame({
      family: YAS_FAMILY_CORE,
      kind: YAS_CORE_HELLO,
      class: YAS_CLASS_REQUEST,
      requestId: 1,
      sensitive: false,
      payload: new Uint8Array(0),
    });
    transport.send(YAS_PREFACE);
    transport.send(hello);
    expect(socket.sent.slice(1)).toEqual([YAS_PREFACE, hello]);
    transport.close();
  });
});
