/** YAS Surface family v1 codecs and browser client. */

import {
  YAS_FAMILY_SURFACE,
  YAS_SURFACE_AXIS,
  YAS_SURFACE_CAPTURE,
  YAS_SURFACE_CLOSE,
  YAS_SURFACE_CLOSE_VIEW,
  YAS_SURFACE_CONFIGURE_VIEW,
  YAS_SURFACE_CREATE_APP_ENDPOINT,
  YAS_SURFACE_RELEASE_APP_ENDPOINT,
  YAS_SURFACE_FOCUS,
  YAS_SURFACE_FRAME,
  YAS_SURFACE_FRAME_ACK,
  YAS_SURFACE_KEY,
  YAS_SURFACE_OPEN_VIEW,
  YAS_SURFACE_POINTER,
  YAS_SURFACE_PREEDIT,
  YAS_SURFACE_REMOTE_INPUT,
  YAS_SURFACE_REMOTE_INPUT_POINTER,
  YAS_SURFACE_REMOTE_INPUT_TOUCH,
  YAS_SURFACE_MAX_REMOTE_CONTACTS,
  YAS_SURFACE_RESET_VIEW,
  YAS_SURFACE_RESIZE,
  YAS_SURFACE_RESIZE_SCALE_120_EXTENSION,
  YAS_SURFACE_STATE,
  YAS_SURFACE_STATE_ACK,
  YAS_SURFACE_TEXT,
  YAS_SURFACE_TOUCH,
  YAS_SURFACE_UNWATCH,
  YAS_SURFACE_VERSION,
  YAS_SURFACE_WATCH,
  YAS_SURFACE_AXIS_FLAGS_MASK,
  YAS_SURFACE_AXIS_SOURCE_WHEEL_TILT,
  YAS_SURFACE_CODEC_AV1_V1,
  YAS_SURFACE_CODEC_H264_V1,
  YAS_SURFACE_CODEC_PNG_V1,
  YAS_SURFACE_CURSOR_CUSTOM,
  YAS_SURFACE_CURSOR_HIDDEN,
  YAS_SURFACE_CURSOR_NAMED,
  YAS_SURFACE_FRAME_FLAGS_MASK,
  YAS_SURFACE_KEY_STATE_REPEAT,
  YAS_SURFACE_MAX_INLINE_CURSOR_BYTES,
  YAS_SURFACE_MAX_APP_ENDPOINTS_PER_SESSION,
  YAS_SURFACE_MAX_APP_ENDPOINT_LIFETIME_NS,
  YAS_SURFACE_MAX_FRAME_RATE,
  YAS_SURFACE_MAX_SURFACES_PER_SESSION,
  YAS_SURFACE_MAX_VIEW_DIMENSION,
  YAS_SURFACE_MAX_VIEW_PIXELS,
  YAS_SURFACE_MAX_VIEWS_PER_SESSION,
  YAS_SURFACE_LIMIT_MAX_APP_ENDPOINTS_PER_SESSION,
  YAS_SURFACE_LIMIT_MAX_APP_ENDPOINT_LIFETIME_NS,
  YAS_SURFACE_LIMIT_MAX_FRAME_RATE,
  YAS_SURFACE_LIMIT_MAX_INLINE_CURSOR_BYTES,
  YAS_SURFACE_LIMIT_MAX_REMOTE_CONTACTS,
  YAS_SURFACE_LIMIT_MAX_SURFACES_PER_SESSION,
  YAS_SURFACE_LIMIT_MAX_VIEW_DIMENSION,
  YAS_SURFACE_LIMIT_MAX_VIEW_PIXELS,
  YAS_SURFACE_LIMIT_MAX_VIEWS_PER_SESSION,
  YAS_SURFACE_MODIFIER_MASK,
  YAS_SURFACE_POINTER_BUTTON_FORWARD,
  YAS_SURFACE_POINTER_PHASE_LEAVE,
  YAS_SURFACE_STATE_ACTIVATION_REVISION_EXTENSION,
  YAS_SURFACE_STATE_CURSOR_EXTENSION,
  YAS_SURFACE_STATE_TEXT_INPUT_EXTENSION,
  YAS_SURFACE_TEXT_INPUT_ENABLED,
  YAS_SURFACE_TEXT_INPUT_FLAGS_MASK,
  YAS_SURFACE_TEXT_INPUT_HAS_CURSOR_RECT,
  YAS_SURFACE_TEXT_INPUT_REQUESTED,
  YAS_SURFACE_TOUCH_PHASE_CANCEL,
  YAS_SURFACE_TOUCH_PHASE_FRAME,
} from "./generated";
import type { YasConnection, YasReceiveBudgetLease } from "./session";
import {
  YAS_STATE_ADD,
  YAS_STATE_DELTA,
  YAS_STATE_PATCH,
  YAS_STATE_REMOVE,
  YAS_STATE_REPLACE,
  YAS_STATE_RESET,
  YAS_STATE_SNAPSHOT_BEGIN,
  YAS_STATE_SNAPSHOT_END,
  YAS_STATE_SNAPSHOT_RECORDS,
  YasStateSubscription,
  YasStateCatalogueRetention,
  detachStateRetainedValue,
  estimateStateRetainedBytes,
  negotiatedStateLimitU32,
  type YasStateBatch,
  type YasWatchOptions,
} from "./state";
import {
  YAS_TRANSFER_MODE_BYTE,
  YAS_TRANSFER_SENDER_TO_RECEIVER,
  decodeInlineOrTransfer,
  transfersFor,
  type YasInlineOrTransfer,
  type YasTransfer,
} from "./transfer";
import {
  YasCursor,
  YasProtocolError,
  YasWriter,
  decodeExtensions,
  encodeExtensions,
  type YasExtension,
  type YasTypedRecord,
} from "./wire";

export {
  YAS_FAMILY_SURFACE,
  YAS_SURFACE_AXIS,
  YAS_SURFACE_CAPTURE,
  YAS_SURFACE_CLOSE,
  YAS_SURFACE_CLOSE_VIEW,
  YAS_SURFACE_CONFIGURE_VIEW,
  YAS_SURFACE_CREATE_APP_ENDPOINT,
  YAS_SURFACE_RELEASE_APP_ENDPOINT,
  YAS_SURFACE_FOCUS,
  YAS_SURFACE_FRAME,
  YAS_SURFACE_FRAME_ACK,
  YAS_SURFACE_KEY,
  YAS_SURFACE_OPEN_VIEW,
  YAS_SURFACE_POINTER,
  YAS_SURFACE_PREEDIT,
  YAS_SURFACE_REMOTE_INPUT,
  YAS_SURFACE_RESET_VIEW,
  YAS_SURFACE_RESIZE,
  YAS_SURFACE_STATE,
  YAS_SURFACE_STATE_ACK,
  YAS_SURFACE_TEXT,
  YAS_SURFACE_TOUCH,
  YAS_SURFACE_UNWATCH,
  YAS_SURFACE_VERSION,
  YAS_SURFACE_WATCH,
} from "./generated";

export interface YasSurfaceRecord {
  surfaceHandle: bigint;
  revision: bigint;
  parentHandle: bigint;
  appHandle: bigint;
  lifecycle: number;
  compositeWidth: number;
  compositeHeight: number;
  logicalWidth32_32: bigint;
  logicalHeight32_32: bigint;
  applicationId: string;
  title: string;
  extensions: readonly YasExtension[];
}

export interface YasSurfaceSnapshot {
  revision: bigint;
  surfaces: readonly YasSurfaceRecord[];
}

export interface YasSurfaceCreateAppEndpoint {
  operationId: Uint8Array;
  applicationId: string;
  extensions?: readonly YasExtension[];
}

export interface YasSurfaceEnvironmentOverride {
  key: Uint8Array;
  value: Uint8Array;
}

export interface YasSurfaceCreateAppEndpointResult {
  appHandle: bigint;
  expiresServerNs: bigint;
  environment: readonly YasSurfaceEnvironmentOverride[];
  extensions: readonly YasExtension[];
}

export interface YasSurfaceReleaseAppEndpoint {
  appHandle: bigint;
  operationId: Uint8Array;
  extensions?: readonly YasExtension[];
}

export interface YasSurfaceLimits {
  maxSurfacesPerSession: number;
  maxViewsPerSession: number;
  maxViewDimension: number;
  maxViewPixels: bigint;
  maxFrameRate: number;
  maxInlineCursorBytes: number;
  maxRemoteContacts: number;
  maxAppEndpointsPerSession: number;
  maxAppEndpointLifetimeNs: bigint;
}

export const YAS_SURFACE_HARD_LIMITS: YasSurfaceLimits = {
  maxSurfacesPerSession: YAS_SURFACE_MAX_SURFACES_PER_SESSION,
  maxViewsPerSession: YAS_SURFACE_MAX_VIEWS_PER_SESSION,
  maxViewDimension: YAS_SURFACE_MAX_VIEW_DIMENSION,
  maxViewPixels: BigInt(YAS_SURFACE_MAX_VIEW_PIXELS),
  maxFrameRate: YAS_SURFACE_MAX_FRAME_RATE,
  maxInlineCursorBytes: YAS_SURFACE_MAX_INLINE_CURSOR_BYTES,
  maxRemoteContacts: YAS_SURFACE_MAX_REMOTE_CONTACTS,
  maxAppEndpointsPerSession: YAS_SURFACE_MAX_APP_ENDPOINTS_PER_SESSION,
  maxAppEndpointLifetimeNs: BigInt(YAS_SURFACE_MAX_APP_ENDPOINT_LIFETIME_NS),
};

export function surfaceLimitsFromExtensions(
  extensions: readonly YasExtension[],
): YasSurfaceLimits {
  const tags = new Set<number>([
    YAS_SURFACE_LIMIT_MAX_SURFACES_PER_SESSION,
    YAS_SURFACE_LIMIT_MAX_VIEWS_PER_SESSION,
    YAS_SURFACE_LIMIT_MAX_VIEW_DIMENSION,
    YAS_SURFACE_LIMIT_MAX_VIEW_PIXELS,
    YAS_SURFACE_LIMIT_MAX_FRAME_RATE,
    YAS_SURFACE_LIMIT_MAX_INLINE_CURSOR_BYTES,
    YAS_SURFACE_LIMIT_MAX_REMOTE_CONTACTS,
    YAS_SURFACE_LIMIT_MAX_APP_ENDPOINTS_PER_SESSION,
    YAS_SURFACE_LIMIT_MAX_APP_ENDPOINT_LIFETIME_NS,
  ]);
  if (
    extensions.some(
      (extension) => extension.required && !tags.has(extension.tag),
    )
  )
    throw new YasProtocolError("unknown required Surface family limit");
  const value = {
    maxSurfacesPerSession: surfaceLimitU32(
      extensions,
      YAS_SURFACE_LIMIT_MAX_SURFACES_PER_SESSION,
    ),
    maxViewsPerSession: surfaceLimitU32(
      extensions,
      YAS_SURFACE_LIMIT_MAX_VIEWS_PER_SESSION,
    ),
    maxViewDimension: surfaceLimitU32(
      extensions,
      YAS_SURFACE_LIMIT_MAX_VIEW_DIMENSION,
    ),
    maxViewPixels: surfaceLimitU64(
      extensions,
      YAS_SURFACE_LIMIT_MAX_VIEW_PIXELS,
    ),
    maxFrameRate: surfaceLimitU32(extensions, YAS_SURFACE_LIMIT_MAX_FRAME_RATE),
    maxInlineCursorBytes: surfaceLimitU32(
      extensions,
      YAS_SURFACE_LIMIT_MAX_INLINE_CURSOR_BYTES,
    ),
    maxRemoteContacts: surfaceLimitU32(
      extensions,
      YAS_SURFACE_LIMIT_MAX_REMOTE_CONTACTS,
    ),
    maxAppEndpointsPerSession: surfaceLimitU32(
      extensions,
      YAS_SURFACE_LIMIT_MAX_APP_ENDPOINTS_PER_SESSION,
    ),
    maxAppEndpointLifetimeNs: surfaceLimitU64(
      extensions,
      YAS_SURFACE_LIMIT_MAX_APP_ENDPOINT_LIFETIME_NS,
    ),
  };
  validateSurfaceLimits(value);
  return value;
}

export function surfaceLimitsExtensions(
  value: YasSurfaceLimits,
): YasExtension[] {
  validateSurfaceLimits(value);
  return [
    surfaceLimit32(
      YAS_SURFACE_LIMIT_MAX_SURFACES_PER_SESSION,
      value.maxSurfacesPerSession,
    ),
    surfaceLimit32(
      YAS_SURFACE_LIMIT_MAX_VIEWS_PER_SESSION,
      value.maxViewsPerSession,
    ),
    surfaceLimit32(
      YAS_SURFACE_LIMIT_MAX_VIEW_DIMENSION,
      value.maxViewDimension,
    ),
    surfaceLimit64(YAS_SURFACE_LIMIT_MAX_VIEW_PIXELS, value.maxViewPixels),
    surfaceLimit32(YAS_SURFACE_LIMIT_MAX_FRAME_RATE, value.maxFrameRate),
    surfaceLimit32(
      YAS_SURFACE_LIMIT_MAX_INLINE_CURSOR_BYTES,
      value.maxInlineCursorBytes,
    ),
    surfaceLimit32(
      YAS_SURFACE_LIMIT_MAX_REMOTE_CONTACTS,
      value.maxRemoteContacts,
    ),
    surfaceLimit32(
      YAS_SURFACE_LIMIT_MAX_APP_ENDPOINTS_PER_SESSION,
      value.maxAppEndpointsPerSession,
    ),
    surfaceLimit64(
      YAS_SURFACE_LIMIT_MAX_APP_ENDPOINT_LIFETIME_NS,
      value.maxAppEndpointLifetimeNs,
    ),
  ];
}

export interface YasSurfaceOpenView {
  surfaceHandle: bigint;
  width: number;
  height: number;
  maxFps: number;
  decoderCapacity: number;
  codecVersions: readonly number[];
  extensions?: readonly YasExtension[];
}

export interface YasSurfaceViewResult {
  viewId: number;
  codecVersion: number;
  maxInflightFrames: number;
  maxEncodedFrame: number;
  maxDecodedFrame: number;
  firstSequence: bigint;
  extensions: readonly YasExtension[];
}

export interface YasSurfaceConfigureView {
  width: number;
  height: number;
  maxFps: number;
  decoderCapacity: number;
  latencyTargetNs: bigint;
  extensions?: readonly YasExtension[];
}

/** Required RESIZE extension carrying the requested Wayland scale in 120ths. */
export function surfaceResizeScale120Extension(scale120: number): YasExtension {
  if (!Number.isInteger(scale120) || scale120 <= 0 || scale120 > 0xffff)
    throw new YasProtocolError("invalid Surface RESIZE scale");
  return {
    tag: YAS_SURFACE_RESIZE_SCALE_120_EXTENSION,
    required: true,
    value: new YasWriter().u16(scale120).finish(),
  };
}

export interface YasSurfaceFrameFeedback {
  presentedSequence: bigint;
  decoderQueueDepth: number;
  availableSlots: number;
}

export interface YasSurfaceFrame {
  viewId: number;
  sequence: bigint;
  baseSequence: bigint;
  captureNs: bigint;
  presentationNs: bigint;
  flags: number;
  codecVersion: number;
  fragmentIndex: number;
  fragmentCount: number;
  completeLength: number;
  payload: Uint8Array;
}

export interface YasSurfaceRemoteContact {
  contactId: number;
  x32_32: bigint;
  y32_32: bigint;
}

export interface YasSurfaceRemoteInput {
  surfaceHandle: bigint;
  seatHandle: bigint;
  expiresServerNs: bigint;
  inputKind: number;
  contacts: readonly YasSurfaceRemoteContact[];
}

export interface YasSurfaceCaptureResult {
  byteLength: bigint;
  contentHash: Uint8Array;
  bytes(): Promise<Uint8Array>;
}

export type YasSurfaceCursorState =
  | { kind: "named"; name: string }
  | { kind: "hidden" }
  | {
      kind: "custom";
      hotspotX: number;
      hotspotY: number;
      width: number;
      height: number;
      scale120: number;
      png: Uint8Array;
    };

export interface YasSurfaceTextInputState {
  enabled: boolean;
  requested: boolean;
  contentHint: number;
  contentPurpose: number;
  cursorRect: null | { x: number; y: number; width: number; height: number };
}

const surfaceStateExtensionTags = new Set([
  YAS_SURFACE_STATE_ACTIVATION_REVISION_EXTENSION,
  YAS_SURFACE_STATE_CURSOR_EXTENSION,
  YAS_SURFACE_STATE_TEXT_INPUT_EXTENSION,
]);

export function encodeSurfaceRecord(value: YasSurfaceRecord): Uint8Array {
  validateSurfaceRecord(value);
  return new YasWriter()
    .u64(value.surfaceHandle)
    .u64(value.revision)
    .u64(value.parentHandle)
    .u64(value.appHandle)
    .u8(value.lifecycle)
    .u8(0)
    .u32(value.compositeWidth)
    .u32(value.compositeHeight)
    .i64(value.logicalWidth32_32)
    .i64(value.logicalHeight32_32)
    .utf8U16(value.applicationId)
    .utf8U16(value.title)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeSurfaceRecord(bytes: Uint8Array): YasSurfaceRecord {
  const cursor = new YasCursor(bytes);
  const value: YasSurfaceRecord = {
    surfaceHandle: cursor.u64("Surface handle"),
    revision: cursor.u64("Surface revision"),
    parentHandle: cursor.u64("Surface parent handle"),
    appHandle: cursor.u64("Surface app handle"),
    lifecycle: cursor.u8("Surface lifecycle"),
    compositeWidth: 0,
    compositeHeight: 0,
    logicalWidth32_32: 0n,
    logicalHeight32_32: 0n,
    applicationId: "",
    title: "",
    extensions: [],
  };
  if (cursor.u8("Surface reserved") !== 0)
    throw new YasProtocolError("Surface record reserved byte is nonzero");
  value.compositeWidth = cursor.u32("Surface composite width");
  value.compositeHeight = cursor.u32("Surface composite height");
  value.logicalWidth32_32 = cursor.i64("Surface logical width");
  value.logicalHeight32_32 = cursor.i64("Surface logical height");
  value.applicationId = cursor.utf8U16("Surface application ID");
  value.title = cursor.utf8U16("Surface title");
  value.extensions = decodeExtensions(
    cursor,
    surfaceStateExtensionTags,
    "Surface extensions",
  );
  cursor.end("Surface record");
  validateSurfaceRecord(value);
  return value;
}

export function surfaceActivationRevision(
  record: YasSurfaceRecord,
): bigint | undefined {
  const extension = record.extensions.find(
    (value) => value.tag === YAS_SURFACE_STATE_ACTIVATION_REVISION_EXTENSION,
  );
  if (!extension) return undefined;
  const cursor = new YasCursor(extension.value);
  const revision = cursor.u64("Surface activation revision");
  cursor.end("Surface activation revision");
  if (revision === 0n)
    throw new YasProtocolError("Surface activation revision is zero");
  return revision;
}

export function surfaceCursorState(
  record: YasSurfaceRecord,
): YasSurfaceCursorState | undefined {
  const extension = record.extensions.find(
    (value) => value.tag === YAS_SURFACE_STATE_CURSOR_EXTENSION,
  );
  return extension ? decodeSurfaceCursorState(extension.value) : undefined;
}

export function decodeSurfaceCursorState(
  bytes: Uint8Array,
): YasSurfaceCursorState {
  const cursor = new YasCursor(bytes);
  const kind = cursor.u8("Surface cursor kind");
  if (cursor.take(3, "Surface cursor reserved").some((value) => value !== 0))
    throw new YasProtocolError("Surface cursor reserved bytes are nonzero");
  let value: YasSurfaceCursorState;
  if (kind === YAS_SURFACE_CURSOR_NAMED) {
    const name = cursor.utf8U16("Surface cursor name");
    if (name.length === 0)
      throw new YasProtocolError("Surface cursor name is empty");
    value = { kind: "named", name };
  } else if (kind === YAS_SURFACE_CURSOR_HIDDEN) {
    value = { kind: "hidden" };
  } else if (kind === YAS_SURFACE_CURSOR_CUSTOM) {
    const hotspotX = cursor.i32("Surface cursor hotspot x");
    const hotspotY = cursor.i32("Surface cursor hotspot y");
    const width = cursor.u32("Surface cursor width");
    const height = cursor.u32("Surface cursor height");
    const scale120 = cursor.u16("Surface cursor scale");
    if (cursor.u16("Surface custom cursor reserved") !== 0)
      throw new YasProtocolError("Surface custom cursor reserved is nonzero");
    const png = new Uint8Array(cursor.bytesU32("Surface cursor PNG"));
    if (
      width === 0 ||
      height === 0 ||
      scale120 === 0 ||
      png.length === 0 ||
      png.length > YAS_SURFACE_MAX_INLINE_CURSOR_BYTES
    )
      throw new YasProtocolError("invalid Surface custom cursor");
    value = {
      kind: "custom",
      hotspotX,
      hotspotY,
      width,
      height,
      scale120,
      png,
    };
  } else {
    throw new YasProtocolError("unknown Surface cursor kind");
  }
  cursor.end("Surface cursor state");
  return value;
}

export function surfaceTextInputState(
  record: YasSurfaceRecord,
): YasSurfaceTextInputState | undefined {
  const extension = record.extensions.find(
    (value) => value.tag === YAS_SURFACE_STATE_TEXT_INPUT_EXTENSION,
  );
  return extension ? decodeSurfaceTextInputState(extension.value) : undefined;
}

export function decodeSurfaceTextInputState(
  bytes: Uint8Array,
): YasSurfaceTextInputState {
  const cursor = new YasCursor(bytes);
  const flags = cursor.u16("Surface text input flags");
  if (
    flags & ~YAS_SURFACE_TEXT_INPUT_FLAGS_MASK ||
    cursor.u16("Surface text input reserved") !== 0
  )
    throw new YasProtocolError("invalid Surface text input flags");
  const enabled = (flags & YAS_SURFACE_TEXT_INPUT_ENABLED) !== 0;
  const requested = (flags & YAS_SURFACE_TEXT_INPUT_REQUESTED) !== 0;
  const contentHint = cursor.u32("Surface text input content hint");
  const contentPurpose = cursor.u32("Surface text input content purpose");
  const cursorRect =
    flags & YAS_SURFACE_TEXT_INPUT_HAS_CURSOR_RECT
      ? {
          x: cursor.i32("Surface text input cursor x"),
          y: cursor.i32("Surface text input cursor y"),
          width: cursor.i32("Surface text input cursor width"),
          height: cursor.i32("Surface text input cursor height"),
        }
      : null;
  cursor.end("Surface text input state");
  if (requested && !enabled)
    throw new YasProtocolError("Surface text input requested while disabled");
  if (cursorRect && (cursorRect.width <= 0 || cursorRect.height <= 0))
    throw new YasProtocolError("invalid Surface text input cursor rectangle");
  return { enabled, requested, contentHint, contentPurpose, cursorRect };
}

export function encodeSurfaceCreateAppEndpoint(
  value: YasSurfaceCreateAppEndpoint,
): Uint8Array {
  requireOperationId(value.operationId, "Surface CREATE_APP_ENDPOINT");
  if (value.applicationId.length === 0)
    throw new YasProtocolError("Surface application ID is empty");
  return new YasWriter()
    .bytes(value.operationId)
    .utf8U16(value.applicationId)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeSurfaceCreateAppEndpoint(
  bytes: Uint8Array,
): YasSurfaceCreateAppEndpoint {
  const cursor = new YasCursor(bytes);
  const value = {
    operationId: new Uint8Array(
      cursor.take(16, "Surface CREATE_APP_ENDPOINT operation ID"),
    ),
    applicationId: cursor.utf8U16("Surface application ID"),
    extensions: decodeExtensions(
      cursor,
      undefined,
      "Surface CREATE_APP_ENDPOINT extensions",
    ),
  };
  cursor.end("Surface CREATE_APP_ENDPOINT");
  encodeSurfaceCreateAppEndpoint(value);
  return value;
}

export function encodeSurfaceCreateAppEndpointResult(
  value: YasSurfaceCreateAppEndpointResult,
): Uint8Array {
  requireHandle(value.appHandle, "Surface app handle");
  if (value.expiresServerNs === 0n)
    throw new YasProtocolError("Surface app endpoint expiry is zero");
  if (value.environment.length > 0xffff)
    throw new YasProtocolError("too many Surface environment overrides");
  const keys = new Set<string>();
  const writer = new YasWriter()
    .u64(value.appHandle)
    .u64(value.expiresServerNs)
    .u16(value.environment.length);
  for (const entry of value.environment) {
    const identity = byteKey(entry.key);
    if (entry.key.length === 0 || keys.has(identity))
      throw new YasProtocolError("invalid Surface environment override key");
    keys.add(identity);
    writer.bytesU16(entry.key).bytesU32(entry.value);
  }
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeSurfaceCreateAppEndpointResult(
  bytes: Uint8Array,
): YasSurfaceCreateAppEndpointResult {
  const cursor = new YasCursor(bytes);
  const appHandle = cursor.u64("Surface app handle");
  const expiresServerNs = cursor.u64("Surface app endpoint expiry");
  const count = cursor.u16("Surface environment override count");
  if (count > Math.floor(cursor.remaining / 6))
    throw new YasProtocolError("invalid Surface environment override count");
  const environment: YasSurfaceEnvironmentOverride[] = [];
  const keys = new Set<string>();
  for (let index = 0; index < count; index++) {
    const key = new Uint8Array(cursor.bytesU16("Surface environment key"));
    const value = new Uint8Array(cursor.bytesU32("Surface environment value"));
    const identity = byteKey(key);
    if (key.length === 0 || keys.has(identity))
      throw new YasProtocolError("invalid Surface environment override key");
    keys.add(identity);
    environment.push({ key, value });
  }
  const extensions = decodeExtensions(
    cursor,
    undefined,
    "Surface endpoint extensions",
  );
  cursor.end("Surface CREATE_APP_ENDPOINT Result");
  requireHandle(appHandle, "Surface app handle");
  if (expiresServerNs === 0n)
    throw new YasProtocolError("Surface app endpoint expiry is zero");
  return { appHandle, expiresServerNs, environment, extensions };
}

export function encodeSurfaceReleaseAppEndpoint(
  value: YasSurfaceReleaseAppEndpoint,
): Uint8Array {
  requireHandle(value.appHandle, "Surface app handle");
  requireOperationId(value.operationId, "Surface RELEASE_APP_ENDPOINT");
  return new YasWriter()
    .u64(value.appHandle)
    .bytes(value.operationId)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeSurfaceReleaseAppEndpoint(
  bytes: Uint8Array,
): YasSurfaceReleaseAppEndpoint {
  const cursor = new YasCursor(bytes);
  const value = {
    appHandle: cursor.u64("Surface app handle"),
    operationId: new Uint8Array(
      cursor.take(16, "Surface RELEASE_APP_ENDPOINT operation ID"),
    ),
    extensions: decodeExtensions(
      cursor,
      undefined,
      "Surface RELEASE_APP_ENDPOINT extensions",
    ),
  };
  cursor.end("Surface RELEASE_APP_ENDPOINT");
  encodeSurfaceReleaseAppEndpoint(value);
  return value;
}

export function encodeSurfaceOpenView(value: YasSurfaceOpenView): Uint8Array {
  requireHandle(value.surfaceHandle, "Surface handle");
  if (
    value.width === 0 ||
    value.height === 0 ||
    value.maxFps === 0 ||
    value.decoderCapacity === 0 ||
    value.codecVersions.length === 0 ||
    value.codecVersions.length > 0xff
  )
    throw new YasProtocolError("invalid Surface OPEN_VIEW parameters");
  let previous = 0;
  const codecs = new YasWriter();
  for (const codec of value.codecVersions) {
    if (codec === 0 || codec <= previous)
      throw new YasProtocolError("Surface codecs are not strictly ordered");
    previous = codec;
    codecs.u16(codec);
  }
  return new YasWriter()
    .u64(value.surfaceHandle)
    .u32(value.width)
    .u32(value.height)
    .u16(value.maxFps)
    .u8(value.decoderCapacity)
    .u8(value.codecVersions.length)
    .bytes(codecs.finish())
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeSurfaceViewResult(
  bytes: Uint8Array,
): YasSurfaceViewResult {
  const cursor = new YasCursor(bytes);
  const value: YasSurfaceViewResult = {
    viewId: cursor.u32("Surface view ID"),
    codecVersion: cursor.u16("Surface codec version"),
    maxInflightFrames: cursor.u16("Surface maximum in-flight frames"),
    maxEncodedFrame: cursor.u32("Surface maximum encoded frame"),
    maxDecodedFrame: cursor.u32("Surface maximum decoded frame"),
    firstSequence: cursor.u64("Surface first sequence"),
    extensions: decodeExtensions(cursor, undefined, "Surface view extensions"),
  };
  cursor.end("Surface OPEN_VIEW Result");
  if (
    value.viewId === 0 ||
    value.codecVersion === 0 ||
    value.maxInflightFrames === 0 ||
    value.maxEncodedFrame === 0 ||
    value.maxDecodedFrame === 0
  )
    throw new YasProtocolError("invalid Surface view result limits");
  return value;
}

export function encodeSurfaceFrameFeedback(
  value: YasSurfaceFrameFeedback,
): Uint8Array {
  return new YasWriter()
    .u64(value.presentedSequence)
    .u16(value.decoderQueueDepth)
    .u16(value.availableSlots)
    .finish();
}

export function decodeSurfaceFrame(bytes: Uint8Array): YasSurfaceFrame {
  const cursor = new YasCursor(bytes);
  const value: YasSurfaceFrame = {
    viewId: cursor.u32("Surface frame view ID"),
    sequence: cursor.u64("Surface frame sequence"),
    baseSequence: cursor.u64("Surface frame base sequence"),
    captureNs: cursor.u64("Surface frame capture time"),
    presentationNs: cursor.u64("Surface frame presentation time"),
    flags: cursor.u16("Surface frame flags"),
    codecVersion: cursor.u16("Surface frame codec"),
    fragmentIndex: cursor.u16("Surface frame fragment index"),
    fragmentCount: cursor.u16("Surface frame fragment count"),
    completeLength: cursor.u32("Surface frame complete length"),
    payload: new Uint8Array(cursor.take(cursor.remaining)),
  };
  validateSurfaceFrame(value);
  return value;
}

export function encodeSurfaceFrame(value: YasSurfaceFrame): Uint8Array {
  validateSurfaceFrame(value);
  return new YasWriter()
    .u32(value.viewId)
    .u64(value.sequence)
    .u64(value.baseSequence)
    .u64(value.captureNs)
    .u64(value.presentationNs)
    .u16(value.flags)
    .u16(value.codecVersion)
    .u16(value.fragmentIndex)
    .u16(value.fragmentCount)
    .u32(value.completeLength)
    .bytes(value.payload)
    .finish();
}

export function decodeSurfaceRemoteInput(
  bytes: Uint8Array,
): YasSurfaceRemoteInput {
  const cursor = new YasCursor(bytes);
  const surfaceHandle = cursor.u64("Surface remote-input surface handle");
  const seatHandle = cursor.u64("Surface remote-input seat handle");
  const expiresServerNs = cursor.u64("Surface remote-input expiry");
  const inputKind = cursor.u8("Surface remote-input kind");
  if (cursor.u8("Surface remote-input reserved") !== 0)
    throw new YasProtocolError(
      "Surface REMOTE_INPUT reserved field is nonzero",
    );
  const count = cursor.u16("Surface remote-input contact count");
  if (
    count > YAS_SURFACE_MAX_REMOTE_CONTACTS ||
    count > Math.floor(cursor.remaining / 20)
  )
    throw new YasProtocolError("invalid Surface remote-input contact count");
  const contacts: YasSurfaceRemoteContact[] = [];
  const ids = new Set<number>();
  for (let index = 0; index < count; index++) {
    const contact = {
      contactId: cursor.u32("Surface remote contact ID"),
      x32_32: cursor.i64("Surface remote contact x"),
      y32_32: cursor.i64("Surface remote contact y"),
    };
    if (ids.has(contact.contactId))
      throw new YasProtocolError("duplicate Surface remote contact");
    ids.add(contact.contactId);
    contacts.push(contact);
  }
  cursor.end("Surface REMOTE_INPUT");
  const value = {
    surfaceHandle,
    seatHandle,
    expiresServerNs,
    inputKind,
    contacts,
  };
  validateSurfaceRemoteInput(value);
  return value;
}

export function encodeSurfaceRemoteInput(
  value: YasSurfaceRemoteInput,
): Uint8Array {
  validateSurfaceRemoteInput(value);
  const writer = new YasWriter()
    .u64(value.surfaceHandle)
    .u64(value.seatHandle)
    .u64(value.expiresServerNs)
    .u8(value.inputKind)
    .u8(0)
    .u16(value.contacts.length);
  for (const contact of value.contacts)
    writer.u32(contact.contactId).i64(contact.x32_32).i64(contact.y32_32);
  return writer.finish();
}

export class YasSurfaceCatalog {
  private current = new Map<bigint, YasSurfaceRecord>();
  private staging: Map<bigint, YasSurfaceRecord> | null = null;
  private retention: YasStateCatalogueRetention<bigint>;
  private stagingRetention: YasStateCatalogueRetention<bigint> | null = null;
  private subscription: YasStateSubscription | null = null;
  private listeners = new Set<(snapshot: YasSurfaceSnapshot) => void>();
  private pendingFirstSnapshots = new Set<(error: unknown) => void>();
  private revision = 0n;
  private readonly removeInvalidation: () => void;
  private pendingWatch: Promise<void> | null = null;
  private pendingWatchCancel: ((error: unknown) => void) | null = null;
  private watchEpoch = 0;
  private disposed = false;

  constructor(private readonly connection: YasConnection) {
    this.retention = YasStateCatalogueRetention.forConnection(connection);
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family === undefined || family === YAS_FAMILY_SURFACE) {
        this.cancelPendingWatch(
          new YasProtocolError("Surface catalogue was invalidated"),
        );
        this.resetLocal();
      }
    });
  }

  get snapshot(): YasSurfaceSnapshot {
    return { revision: this.revision, surfaces: [...this.current.values()] };
  }

  subscribe(listener: (snapshot: YasSurfaceSnapshot) => void): () => void {
    if (this.disposed) throw new Error("Surface catalogue is disposed");
    this.listeners.add(listener);
    try {
      listener(this.snapshot);
    } catch {
      this.listeners.delete(listener);
    }
    return () => this.listeners.delete(listener);
  }

  async firstSnapshot(
    options: YasWatchOptions = {},
  ): Promise<YasSurfaceSnapshot> {
    if (this.disposed) throw new Error("Surface catalogue is disposed");
    if (this.revision !== 0n && this.subscription?.active) return this.snapshot;
    let remove: (() => void) | undefined;
    let rejectPending!: (error: unknown) => void;
    const result = new Promise<YasSurfaceSnapshot>((resolve, reject) => {
      rejectPending = (error) => {
        this.pendingFirstSnapshots.delete(rejectPending);
        remove?.();
        reject(error);
      };
      this.pendingFirstSnapshots.add(rejectPending);
      remove = this.subscribe((snapshot) => {
        if (snapshot.revision === 0n) return;
        this.pendingFirstSnapshots.delete(rejectPending);
        remove?.();
        resolve(snapshot);
      });
    });
    try {
      return await Promise.race([
        result,
        this.watch(options).then(() => result),
      ]);
    } finally {
      remove?.();
      this.pendingFirstSnapshots.delete(rejectPending);
    }
  }

  watch(options: YasWatchOptions = {}): Promise<void> {
    if (this.disposed)
      return Promise.reject(new Error("Surface catalogue is disposed"));
    if (this.subscription?.active) return Promise.resolve();
    if (this.pendingWatch) return this.pendingWatch;
    this.resetLocal();
    const epoch = this.watchEpoch;
    const watched = YasStateSubscription.watch(
      this.connection,
      YAS_FAMILY_SURFACE,
      YAS_SURFACE_WATCH,
      YAS_SURFACE_UNWATCH,
      YAS_SURFACE_STATE,
      YAS_SURFACE_STATE_ACK,
      options,
      (batch) => {
        if (!this.disposed && epoch === this.watchEpoch) this.apply(batch);
      },
    ).then(async (subscription) => {
      if (this.disposed || epoch !== this.watchEpoch) {
        await subscription.unwatch().catch(() => undefined);
        throw new YasProtocolError("Surface catalogue watch was cancelled");
      }
      this.subscription = subscription;
    });
    let cancel!: (error: unknown) => void;
    const cancelled = new Promise<never>((_, reject) => {
      cancel = reject;
    });
    let pending!: Promise<void>;
    pending = Promise.race([watched, cancelled]).finally(() => {
      if (this.pendingWatch !== pending) return;
      this.pendingWatch = null;
      if (this.pendingWatchCancel === cancel) this.pendingWatchCancel = null;
    });
    this.pendingWatch = pending;
    this.pendingWatchCancel = cancel;
    return pending;
  }

  async unwatch(): Promise<void> {
    this.cancelPendingWatch(
      new YasProtocolError("Surface catalogue watch was cancelled"),
    );
    const subscription = this.subscription;
    this.subscription = null;
    if (!this.disposed) this.clearState();
    await subscription?.unwatch();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    const disposalError = new Error("Surface catalogue is disposed");
    this.cancelPendingWatch(disposalError);
    this.removeInvalidation();
    for (const reject of [...this.pendingFirstSnapshots]) reject(disposalError);
    this.pendingFirstSnapshots.clear();
    this.listeners.clear();
    const subscription = this.subscription;
    this.subscription = null;
    this.retention.dispose();
    this.stagingRetention?.dispose();
    this.current.clear();
    this.staging = null;
    this.stagingRetention = null;
    void subscription?.unwatch().catch(() => undefined);
  }

  private apply(batch: YasStateBatch): void {
    if (this.disposed) return;
    if (batch.phase === YAS_STATE_RESET) {
      this.clearState();
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_BEGIN) {
      this.discardStaging();
      this.staging = new Map();
      this.stagingRetention = YasStateCatalogueRetention.forConnection(
        this.connection,
      );
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_RECORDS) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("Surface snapshot records without begin");
      try {
        this.applyRecords(this.staging, this.stagingRetention, batch.records);
        this.validateCatalog(this.staging);
      } catch (error) {
        this.discardStaging();
        throw error;
      }
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_END) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("Surface snapshot end without begin");
      try {
        this.applyRecords(this.staging, this.stagingRetention, batch.records);
        this.validateCatalog(this.staging);
      } catch (error) {
        this.discardStaging();
        throw error;
      }
      const previousRetention = this.retention;
      this.current = this.staging;
      this.retention = this.stagingRetention;
      this.staging = null;
      this.stagingRetention = null;
      previousRetention.dispose();
      this.revision = batch.toRevision;
      this.emit();
      return;
    }
    if (batch.phase === YAS_STATE_DELTA) {
      const retention = this.retention.clone();
      let next: Map<bigint, YasSurfaceRecord>;
      try {
        next = new Map(this.current);
        this.applyRecords(next, retention, batch.records);
        this.validateCatalog(next);
      } catch (error) {
        retention.dispose();
        throw error;
      }
      const previousRetention = this.retention;
      this.current = next;
      this.retention = retention;
      previousRetention.dispose();
      this.revision = batch.toRevision;
      this.emit();
    }
  }

  private applyRecords(
    target: Map<bigint, YasSurfaceRecord>,
    retention: YasStateCatalogueRetention<bigint>,
    records: readonly YasTypedRecord[],
  ): void {
    for (const action of records) {
      if (action.kind === YAS_STATE_ADD || action.kind === YAS_STATE_REPLACE) {
        const record = detachStateRetainedValue(
          decodeSurfaceRecord(action.body),
        );
        const exists = target.has(record.surfaceHandle);
        if ((action.kind === YAS_STATE_ADD) === exists)
          throw new YasProtocolError("Surface ADD/REPLACE precondition failed");
        if (action.kind === YAS_STATE_ADD && target.size >= this.catalogLimit())
          throw new YasProtocolError(
            "Surface catalogue exceeds its negotiated surface limit",
          );
        retention.upsert(
          record.surfaceHandle,
          Math.max(
            encodeSurfaceRecord(record).length,
            estimateStateRetainedBytes(record),
          ),
        );
        target.set(record.surfaceHandle, record);
      } else if (action.kind === YAS_STATE_PATCH) {
        const cursor = new YasCursor(action.body);
        const handle = cursor.u64("patched Surface handle");
        const revision = cursor.u64("patched Surface revision");
        const extensions = decodeExtensions(
          cursor,
          surfaceStateExtensionTags,
          "Surface PATCH extensions",
        );
        cursor.end("Surface PATCH");
        requireHandle(handle, "patched Surface handle");
        if (revision === 0n)
          throw new YasProtocolError("patched Surface revision is zero");
        const previous = target.get(handle);
        if (!previous)
          throw new YasProtocolError("Surface PATCH names an unknown handle");
        const next = detachStateRetainedValue({
          ...previous,
          revision,
          extensions: mergeExtensions(previous.extensions, extensions),
        });
        validateSurfaceRecord(next);
        retention.upsert(
          handle,
          Math.max(
            encodeSurfaceRecord(next).length,
            estimateStateRetainedBytes(next),
          ),
        );
        target.set(handle, next);
      } else if (action.kind === YAS_STATE_REMOVE) {
        const cursor = new YasCursor(action.body);
        const handle = cursor.u64("removed Surface handle");
        const revision = cursor.u64("removed Surface revision");
        cursor.end("Surface REMOVE");
        requireHandle(handle, "removed Surface handle");
        if (revision === 0n)
          throw new YasProtocolError("removed Surface revision is zero");
        if (!target.has(handle))
          throw new YasProtocolError("Surface REMOVE names an unknown handle");
        retention.remove(handle);
        target.delete(handle);
      }
    }
  }

  private validateCatalog(
    records: ReadonlyMap<bigint, YasSurfaceRecord>,
  ): void {
    if (records.size > this.catalogLimit())
      throw new YasProtocolError(
        "Surface catalogue exceeds its negotiated surface limit",
      );
  }

  private catalogLimit(): number {
    return negotiatedStateLimitU32(
      this.connection,
      YAS_FAMILY_SURFACE,
      YAS_SURFACE_VERSION,
      YAS_SURFACE_LIMIT_MAX_SURFACES_PER_SESSION,
      YAS_SURFACE_MAX_SURFACES_PER_SESSION,
    );
  }

  private resetLocal(): void {
    if (this.disposed) return;
    this.subscription = null;
    this.clearState();
  }

  private cancelPendingWatch(error: unknown): void {
    this.watchEpoch++;
    const cancel = this.pendingWatchCancel;
    this.pendingWatch = null;
    this.pendingWatchCancel = null;
    cancel?.(error);
    for (const reject of [...this.pendingFirstSnapshots]) reject(error);
    this.pendingFirstSnapshots.clear();
  }

  private clearState(): void {
    this.retention.dispose();
    this.stagingRetention?.dispose();
    this.staging = null;
    this.retention = YasStateCatalogueRetention.forConnection(this.connection);
    this.stagingRetention = null;
    this.current = new Map();
    this.revision = 0n;
    this.emit();
  }

  private discardStaging(): void {
    this.stagingRetention?.dispose();
    this.staging = null;
    this.stagingRetention = null;
  }

  private emit(): void {
    const snapshot = this.snapshot;
    for (const listener of this.listeners) {
      try {
        listener(snapshot);
      } catch {
        // One observer cannot block sibling delivery or wire cleanup.
      }
    }
  }
}

interface SurfaceFrameAssembly {
  template: YasSurfaceFrame;
  datagram: boolean;
  nextFragment: number;
  chunks: Uint8Array[];
  received: number;
}

export class YasSurfaceView {
  private assemblies = new Map<bigint, SurfaceFrameAssembly>();
  private listeners = new Set<(frame: YasSurfaceFrame) => void>();
  private pending: YasSurfaceFrame[] = [];
  private retainedBytes = 0;
  private highestReceived: bigint;
  private highestPresented: bigint;
  private closed = false;
  private configureQueue: Promise<void> = Promise.resolve();

  constructor(
    private readonly client: YasSurfaceClient,
    readonly result: YasSurfaceViewResult,
    private readonly lease: YasReceiveBudgetLease,
  ) {
    this.highestReceived = result.firstSequence - 1n;
    this.highestPresented = result.firstSequence - 1n;
  }

  subscribe(listener: (frame: YasSurfaceFrame) => void): () => void {
    this.listeners.add(listener);
    const pending = this.pending.splice(0);
    for (const frame of pending) listener(frame);
    return () => this.listeners.delete(listener);
  }

  acknowledge(feedback: YasSurfaceFrameFeedback): void {
    this.recordFeedback(feedback);
    this.client.connection.sendEvent(
      YAS_FAMILY_SURFACE,
      YAS_SURFACE_FRAME_ACK,
      new YasWriter()
        .u32(this.result.viewId)
        .bytes(encodeSurfaceFrameFeedback(feedback))
        .finish(),
    );
  }

  recordFeedback(feedback: YasSurfaceFrameFeedback): void {
    if (
      feedback.presentedSequence < this.highestPresented ||
      feedback.presentedSequence > this.highestReceived
    )
      throw new YasProtocolError(
        "Surface feedback acknowledges an unseen frame",
      );
    this.highestPresented = feedback.presentedSequence;
  }

  configure(value: YasSurfaceConfigureView): Promise<void> {
    const configured = this.configureQueue.then(() => this.configureNow(value));
    this.configureQueue = configured.catch(() => undefined);
    return configured;
  }

  private async configureNow(value: YasSurfaceConfigureView): Promise<void> {
    if (this.closed) throw new YasProtocolError("Surface view is closed");
    if (
      value.width === 0 ||
      value.height === 0 ||
      value.maxFps === 0 ||
      value.decoderCapacity === 0
    )
      throw new YasProtocolError("invalid Surface CONFIGURE_VIEW parameters");
    const previous = this.lease.bytes;
    const perFrame = BigInt(this.result.maxEncodedFrame);
    const next = perFrame * BigInt(value.decoderCapacity);
    if (next > previous) this.lease.resizeExact(next);
    try {
      await this.client.connection.requestDecoded(
        YAS_FAMILY_SURFACE,
        YAS_SURFACE_CONFIGURE_VIEW,
        new YasWriter()
          .u32(this.result.viewId)
          .u32(value.width)
          .u32(value.height)
          .u16(value.maxFps)
          .u8(value.decoderCapacity)
          .u8(0)
          .u64(value.latencyTargetNs)
          .bytes(encodeExtensions(value.extensions))
          .finish(),
        (body) => {
          new YasCursor(body).end("Surface CONFIGURE_VIEW Result");
          if (!this.closed)
            this.result.maxInflightFrames = value.decoderCapacity;
        },
      );
    } catch (error) {
      if (!this.closed && next > previous) this.lease.resizeExact(previous);
      throw error;
    }
    if (this.closed) return;
  }

  reset(): Promise<Uint8Array> {
    return this.client.connection.request(
      YAS_FAMILY_SURFACE,
      YAS_SURFACE_RESET_VIEW,
      new YasWriter().u32(this.result.viewId).finish(),
    );
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    try {
      await this.client.connection.request(
        YAS_FAMILY_SURFACE,
        YAS_SURFACE_CLOSE_VIEW,
        new YasWriter().u32(this.result.viewId).finish(),
      );
    } finally {
      this.closeLocal();
    }
  }

  closeLocal(): void {
    if (this.closed && !this.client.hasView(this.result.viewId)) return;
    this.closed = true;
    this.assemblies.clear();
    this.pending = [];
    this.retainedBytes = 0;
    this.listeners.clear();
    this.client.removeView(this.result.viewId);
    this.lease.release();
  }

  accept(fragment: YasSurfaceFrame, datagram = false): void {
    if (this.closed) return;
    if (
      fragment.codecVersion !== this.result.codecVersion ||
      fragment.completeLength > this.result.maxEncodedFrame
    ) {
      if (datagram) {
        this.discardAssembly(fragment.sequence);
        return;
      }
      throw new YasProtocolError(
        "Surface frame violates negotiated view limits",
      );
    }
    if (fragment.sequence < this.result.firstSequence) {
      if (datagram) return;
      throw new YasProtocolError("Surface frame predates the view sequence");
    }
    // The reliable stream and datagram lane are ordered independently. A
    // reliable frame can therefore finish after a newer datagram even though
    // each lane is internally well behaved. Never deliver a completed
    // sequence twice or let observers' receive/feedback high-water marks move
    // backwards; discard any partial assembly retained for the overtaken
    // frame as well.
    if (fragment.sequence <= this.highestReceived) {
      this.discardAssembly(fragment.sequence);
      return;
    }
    if (
      fragment.sequence >
      this.highestPresented + BigInt(this.result.maxInflightFrames)
    ) {
      if (datagram) return;
      throw new YasProtocolError(
        "Surface sender exceeded its in-flight window",
      );
    }
    let assembly = this.assemblies.get(fragment.sequence);
    if (assembly && assembly.datagram !== datagram) {
      if (datagram) return;
      this.discardAssembly(fragment.sequence);
      assembly = undefined;
    }
    if (!assembly) {
      if (fragment.fragmentIndex !== 0) {
        if (datagram) return;
        throw new YasProtocolError(
          "Surface frame starts with a later fragment",
        );
      }
      this.discardOlderDatagramAssemblies(fragment.sequence);
      if (this.assemblies.size >= this.result.maxInflightFrames) {
        if (datagram) return;
        throw new YasProtocolError("too many Surface frame assemblies");
      }
      assembly = {
        template: fragment,
        datagram,
        nextFragment: 0,
        chunks: [],
        received: 0,
      };
      this.assemblies.set(fragment.sequence, assembly);
    }
    const original = assembly.template;
    if (assembly.nextFragment !== fragment.fragmentIndex) {
      if (assembly.datagram) {
        if (fragment.fragmentIndex > assembly.nextFragment)
          this.discardAssembly(fragment.sequence);
        return;
      }
      throw new YasProtocolError("inconsistent Surface frame fragments");
    }
    if (
      original.fragmentCount !== fragment.fragmentCount ||
      original.completeLength !== fragment.completeLength ||
      original.baseSequence !== fragment.baseSequence ||
      original.captureNs !== fragment.captureNs ||
      original.presentationNs !== fragment.presentationNs ||
      original.flags !== fragment.flags ||
      original.codecVersion !== fragment.codecVersion
    ) {
      if (assembly.datagram) {
        this.discardAssembly(fragment.sequence);
        return;
      }
      throw new YasProtocolError("inconsistent Surface frame fragments");
    }
    assembly.nextFragment++;
    assembly.received += fragment.payload.length;
    this.retainedBytes += fragment.payload.length;
    const remaining = fragment.fragmentCount - assembly.nextFragment;
    if (
      assembly.received > fragment.completeLength ||
      assembly.received + remaining > fragment.completeLength ||
      BigInt(this.retainedBytes) > this.lease.bytes
    ) {
      if (assembly.datagram) {
        this.discardAssembly(fragment.sequence);
        return;
      }
      throw new YasProtocolError("Surface fragments exceed their allocation");
    }
    assembly.chunks.push(fragment.payload);
    if (assembly.nextFragment !== fragment.fragmentCount) return;
    this.assemblies.delete(fragment.sequence);
    this.retainedBytes -= assembly.received;
    if (assembly.received !== fragment.completeLength) {
      if (assembly.datagram) return;
      throw new YasProtocolError("Surface frame has the wrong complete length");
    }
    const frame = {
      ...original,
      fragmentIndex: 0,
      fragmentCount: 1,
      payload: concat(assembly.chunks, assembly.received),
    };
    if (this.listeners.size === 0) {
      if (this.pending.length >= this.result.maxInflightFrames) {
        if (assembly.datagram) return;
        throw new YasProtocolError("too many queued Surface frames");
      }
      this.pending.push(frame);
    }
    // An observer may synchronously acknowledge from inside its frame callback.
    // Publish the receive high-water mark first, as Terminal views do, or that
    // valid ACK is misclassified as feedback for a frame we have not seen.
    if (frame.sequence > this.highestReceived)
      this.highestReceived = frame.sequence;
    if (this.listeners.size > 0)
      for (const listener of this.listeners) listener(frame);
  }

  private discardAssembly(sequence: bigint): void {
    const assembly = this.assemblies.get(sequence);
    if (!assembly) return;
    this.assemblies.delete(sequence);
    this.retainedBytes -= assembly.received;
  }

  private discardOlderDatagramAssemblies(sequence: bigint): void {
    for (const [candidate, assembly] of this.assemblies)
      if (candidate < sequence && assembly.datagram)
        this.discardAssembly(candidate);
  }
}

export class YasSurfaceClient {
  readonly catalog: YasSurfaceCatalog;
  private readonly transfers;
  private readonly views = new Map<number, YasSurfaceView>();
  private readonly appEndpoints = new Map<
    bigint,
    { expiresServerNs: bigint; operationKey: string }
  >();
  private readonly appEndpointOperations = new Map<
    string,
    {
      payloadKey: string;
      pending: Promise<YasSurfaceCreateAppEndpointResult> | null;
      result: YasSurfaceCreateAppEndpointResult | null;
      retainPayload: boolean;
    }
  >();
  private pendingAppEndpoints = 0;
  private endpointGeneration = 0;
  private viewGeneration = 0;
  private removeListeners: (() => void)[];
  private remoteListeners = new Set<(event: YasSurfaceRemoteInput) => void>();
  private disposed = false;

  constructor(readonly connection: YasConnection) {
    connection.family(YAS_FAMILY_SURFACE, YAS_SURFACE_VERSION);
    connection.registerFamilyLimitValidator(
      YAS_FAMILY_SURFACE,
      surfaceLimitsFromExtensions,
    );
    this.catalog = new YasSurfaceCatalog(connection);
    this.transfers = transfersFor(connection);
    this.removeListeners = [
      connection.onEvent(
        YAS_FAMILY_SURFACE,
        YAS_SURFACE_FRAME,
        ({ payload, datagram }) => {
          const frame = decodeSurfaceFrame(payload);
          this.views.get(frame.viewId)?.accept(frame, datagram);
        },
      ),
      connection.onEvent(
        YAS_FAMILY_SURFACE,
        YAS_SURFACE_REMOTE_INPUT,
        ({ payload }) => {
          const event = decodeSurfaceRemoteInput(payload);
          for (const listener of this.remoteListeners) listener(event);
        },
      ),
      connection.onInvalidation(({ family }) => {
        if (family !== undefined && family !== YAS_FAMILY_SURFACE) return;
        for (const view of [...this.views.values()]) {
          this.closeViewRemote(view.result.viewId);
          view.closeLocal();
        }
        for (const handle of this.appEndpoints.keys())
          this.releaseAppEndpointRemote(handle);
        for (const handle of [...this.appEndpoints.keys()])
          this.tombstoneAppEndpoint(handle);
        this.endpointGeneration++;
        this.viewGeneration++;
      }),
    ];
  }

  get limits(): YasSurfaceLimits {
    return surfaceLimitsFromExtensions(
      this.connection.family(YAS_FAMILY_SURFACE, YAS_SURFACE_VERSION).limits,
    );
  }

  list(options: YasWatchOptions = {}): Promise<YasSurfaceSnapshot> {
    return this.catalog.firstSnapshot(options);
  }

  onRemoteInput(listener: (event: YasSurfaceRemoteInput) => void): () => void {
    this.remoteListeners.add(listener);
    return () => this.remoteListeners.delete(listener);
  }

  async createAppEndpoint(
    value: YasSurfaceCreateAppEndpoint,
  ): Promise<YasSurfaceCreateAppEndpointResult> {
    if (this.disposed) throw new YasProtocolError("Surface client is disposed");
    this.pruneExpiredAppEndpoints();
    const payload = encodeSurfaceCreateAppEndpoint(value);
    const operationKey = byteKey(value.operationId);
    const payloadKey = byteKey(payload);
    let operation = this.appEndpointOperations.get(operationKey);
    if (operation) {
      if (operation.payloadKey !== payloadKey)
        throw new YasProtocolError(
          "Surface CREATE_APP_ENDPOINT operation ID was reused with a different payload",
        );
      if (operation.pending) return await operation.pending;
      if (operation.result) {
        const owned = this.appEndpoints.get(operation.result.appHandle);
        if (owned?.operationKey === operationKey) return operation.result;
        operation.result = null;
      }
    }
    const limits = this.limits;
    if (
      this.appEndpoints.size + this.pendingAppEndpoints >=
      limits.maxAppEndpointsPerSession
    )
      throw new YasProtocolError(
        "Surface CREATE_APP_ENDPOINT exceeds the negotiated live-endpoint cap",
      );
    operation ??= {
      payloadKey,
      pending: null,
      result: null,
      retainPayload: false,
    };
    this.appEndpointOperations.set(operationKey, operation);
    const generation = this.endpointGeneration;
    this.pendingAppEndpoints++;
    let pending: Promise<YasSurfaceCreateAppEndpointResult> | null = null;
    try {
      pending = this.connection
        .requestDecoded(
          YAS_FAMILY_SURFACE,
          YAS_SURFACE_CREATE_APP_ENDPOINT,
          payload,
          (body) => {
            const decoded = decodeSurfaceCreateAppEndpointResult(body);
            const remaining = this.connection.nanosecondsUntilServerTime(
              decoded.expiresServerNs,
            );
            if (remaining === 0n || remaining > limits.maxAppEndpointLifetimeNs)
              throw new YasProtocolError(
                "Surface app endpoint expiry exceeds negotiated lifetime",
              );
            return decoded;
          },
        )
        .then((result) => {
          if (this.disposed || generation !== this.endpointGeneration) {
            this.releaseAppEndpointRemote(result.appHandle);
            throw new YasProtocolError(
              "Surface app endpoint completed after family invalidation",
            );
          }
          if (this.appEndpoints.has(result.appHandle))
            throw new YasProtocolError("Surface app handle was reused");
          this.appEndpoints.set(result.appHandle, {
            expiresServerNs: result.expiresServerNs,
            operationKey,
          });
          operation.result = result;
          operation.retainPayload = true;
          return result;
        });
      operation.pending = pending;
      return await pending;
    } finally {
      if (pending && operation.pending === pending) {
        operation.pending = null;
        if (
          !operation.result &&
          !operation.retainPayload &&
          this.appEndpointOperations.get(operationKey) === operation
        )
          this.appEndpointOperations.delete(operationKey);
      }
      this.pendingAppEndpoints--;
    }
  }

  async releaseAppEndpoint(value: YasSurfaceReleaseAppEndpoint): Promise<void> {
    await this.connection.requestDecoded(
      YAS_FAMILY_SURFACE,
      YAS_SURFACE_RELEASE_APP_ENDPOINT,
      encodeSurfaceReleaseAppEndpoint(value),
      (body) => {
        new YasCursor(body).end("Surface RELEASE_APP_ENDPOINT Result");
      },
    );
    this.tombstoneAppEndpoint(value.appHandle);
  }

  async openView(value: YasSurfaceOpenView): Promise<YasSurfaceView> {
    if (this.disposed) throw new YasProtocolError("Surface client is disposed");
    const limits = this.limits;
    if (
      value.width > limits.maxViewDimension ||
      value.height > limits.maxViewDimension ||
      BigInt(value.width) * BigInt(value.height) > limits.maxViewPixels ||
      value.maxFps > limits.maxFrameRate ||
      this.views.size >= limits.maxViewsPerSession
    )
      throw new YasProtocolError("Surface OPEN_VIEW exceeds negotiated limits");
    const generation = this.viewGeneration;
    const outcome = await this.connection.requestDecoded<
      | { view: YasSurfaceView }
      | { result: YasSurfaceViewResult; error: unknown }
    >(
      YAS_FAMILY_SURFACE,
      YAS_SURFACE_OPEN_VIEW,
      encodeSurfaceOpenView(value),
      (body) => {
        const result = decodeSurfaceViewResult(body);
        let lease: YasReceiveBudgetLease | null = null;
        try {
          if (!value.codecVersions.includes(result.codecVersion))
            throw new YasProtocolError(
              "server selected an unoffered Surface codec",
            );
          if (this.disposed || generation !== this.viewGeneration)
            throw new YasProtocolError(
              "Surface OPEN_VIEW completed after client disposal or family invalidation",
            );
          // The aggregate receive budget covers buffered protocol bytes, and
          // for Surface those are the encoded fragments this view reassembles
          // -- `maxEncodedFrame` is the limit `acceptFragment` enforces on
          // them. A decoded image never enters the receive path, so charging
          // the budget for `maxDecodedFrame` reserved memory nothing holds.
          const maximum =
            BigInt(result.maxEncodedFrame) * BigInt(result.maxInflightFrames);
          lease = this.connection.receiveBudget.reserve(maximum, maximum);
          if (this.views.has(result.viewId))
            throw new YasProtocolError("Surface view ID was reused");
          const view = new YasSurfaceView(this, result, lease);
          this.views.set(result.viewId, view);
          return { view };
        } catch (error) {
          lease?.release();
          return { result, error };
        }
      },
    );
    if ("error" in outcome) {
      // OPEN_VIEW has already allocated the peer resource. Any local
      // admission failure must close it, including receive-budget pressure.
      this.closeViewRemote(outcome.result.viewId);
      throw outcome.error;
    }
    if (
      this.disposed ||
      generation !== this.viewGeneration ||
      this.views.get(outcome.view.result.viewId) !== outcome.view
    ) {
      this.closeViewRemote(outcome.view.result.viewId);
      outcome.view.closeLocal();
      throw new YasProtocolError(
        "Surface OPEN_VIEW was invalidated before completion",
      );
    }
    return outcome.view;
  }

  async capture(
    surfaceHandle: bigint,
    revision: bigint,
    formats: readonly number[],
    initialReceiveCredit = 4n * 1024n * 1024n,
    extensions: readonly YasExtension[] = [],
  ): Promise<YasSurfaceCaptureResult> {
    requireHandle(surfaceHandle, "Surface CAPTURE handle");
    if (revision === 0n || formats.length === 0 || formats.length > 0xff)
      throw new YasProtocolError("invalid Surface CAPTURE parameters");
    const lease = this.transfers.reserveReceiveCredit(
      initialReceiveCredit,
      32n * 1024n,
    );
    let accepted = false;
    try {
      return await this.connection.requestDecoded(
        YAS_FAMILY_SURFACE,
        YAS_SURFACE_CAPTURE,
        new YasWriter()
          .u64(surfaceHandle)
          .u64(revision)
          .u64(lease.bytes)
          .u8(formats.length)
          .bytes(new Uint8Array(3))
          .bytes(Uint8Array.from(formats))
          .bytes(encodeExtensions(extensions))
          .finish(),
        (body) => {
          const result = decodeInlineOrTransfer(body);
          if (result.delivery === "inline") {
            lease.release();
            accepted = true;
            const bytes = new Uint8Array(result.bytes);
            return {
              byteLength: result.byteLength,
              contentHash: result.contentHash,
              bytes: async () => new Uint8Array(bytes),
            };
          }
          validateCaptureTransfer(result);
          const transfer = this.transfers.acceptServerDescriptor(
            result.descriptor,
            lease,
          );
          accepted = true;
          return transferCapture(result, transfer);
        },
      );
    } catch (error) {
      if (!accepted) lease.release();
      throw error;
    }
  }

  resize(
    surfaceHandle: bigint,
    operationId: Uint8Array,
    logicalWidth32_32: bigint,
    logicalHeight32_32: bigint,
    extensions: readonly YasExtension[] = [],
  ): Promise<bigint> {
    requireHandle(surfaceHandle, "Surface RESIZE handle");
    requireOperationId(operationId, "Surface RESIZE");
    const releasesClaim = logicalWidth32_32 === 0n && logicalHeight32_32 === 0n;
    if (!releasesClaim && (logicalWidth32_32 <= 0n || logicalHeight32_32 <= 0n))
      throw new YasProtocolError("invalid Surface RESIZE dimensions");
    return this.revisionRequest(
      YAS_SURFACE_RESIZE,
      new YasWriter()
        .u64(surfaceHandle)
        .bytes(operationId)
        .i64(logicalWidth32_32)
        .i64(logicalHeight32_32)
        .bytes(encodeExtensions(extensions))
        .finish(),
    );
  }

  focus(
    surfaceHandle: bigint,
    operationId: Uint8Array,
    focused: boolean,
    extensions: readonly YasExtension[] = [],
  ): Promise<bigint> {
    requireHandle(surfaceHandle, "Surface FOCUS handle");
    requireOperationId(operationId, "Surface FOCUS");
    return this.revisionRequest(
      YAS_SURFACE_FOCUS,
      new YasWriter()
        .u64(surfaceHandle)
        .bytes(operationId)
        .u8(focused ? 1 : 0)
        .bytes(new Uint8Array(7))
        .bytes(encodeExtensions(extensions))
        .finish(),
    );
  }

  async close(
    surfaceHandle: bigint,
    operationId: Uint8Array,
    extensions: readonly YasExtension[] = [],
  ): Promise<void> {
    requireHandle(surfaceHandle, "Surface CLOSE handle");
    requireOperationId(operationId, "Surface CLOSE");
    await this.connection.request(
      YAS_FAMILY_SURFACE,
      YAS_SURFACE_CLOSE,
      new YasWriter()
        .u64(surfaceHandle)
        .bytes(operationId)
        .bytes(encodeExtensions(extensions))
        .finish(),
    );
  }

  key(
    view: YasSurfaceView,
    feedback: YasSurfaceFrameFeedback,
    event: {
      clientMonotonicNs: bigint;
      keyCode: number;
      state: number;
      modifiers: number;
    },
  ): void {
    if (
      event.keyCode === 0 ||
      event.state > YAS_SURFACE_KEY_STATE_REPEAT ||
      event.modifiers & ~YAS_SURFACE_MODIFIER_MASK
    )
      throw new YasProtocolError("invalid Surface KEY event");
    this.withFeedback(view, feedback, YAS_SURFACE_KEY, (writer) =>
      writer
        .u64(event.clientMonotonicNs)
        .u16(event.keyCode)
        .u8(event.state)
        .u8(0)
        .u32(event.modifiers),
    );
  }

  text(
    view: YasSurfaceView,
    feedback: YasSurfaceFrameFeedback,
    clientMonotonicNs: bigint,
    value: string,
  ): void {
    this.withFeedback(view, feedback, YAS_SURFACE_TEXT, (writer) =>
      writer.u64(clientMonotonicNs).utf8U32(value),
    );
  }

  preedit(
    view: YasSurfaceView,
    event: {
      clientMonotonicNs: bigint;
      selectionStart: number;
      selectionEnd: number;
      cursor: number;
      text: string;
    },
  ): void {
    const length = new TextEncoder().encode(event.text).length;
    if (
      event.selectionStart > event.selectionEnd ||
      event.selectionEnd > length ||
      event.cursor > length
    )
      throw new YasProtocolError("invalid Surface PREEDIT range");
    this.connection.sendEvent(
      YAS_FAMILY_SURFACE,
      YAS_SURFACE_PREEDIT,
      new YasWriter()
        .u32(view.result.viewId)
        .u64(event.clientMonotonicNs)
        .u32(event.selectionStart)
        .u32(event.selectionEnd)
        .u32(event.cursor)
        .utf8U32(event.text)
        .finish(),
    );
  }

  pointer(
    view: YasSurfaceView,
    feedback: YasSurfaceFrameFeedback,
    event: {
      clientMonotonicNs: bigint;
      phase: number;
      button: number;
      x32_32: bigint;
      y32_32: bigint;
    },
  ): void {
    if (
      event.phase > YAS_SURFACE_POINTER_PHASE_LEAVE ||
      event.button > YAS_SURFACE_POINTER_BUTTON_FORWARD
    )
      throw new YasProtocolError("invalid Surface POINTER event");
    this.withFeedback(view, feedback, YAS_SURFACE_POINTER, (writer) =>
      writer
        .u64(event.clientMonotonicNs)
        .u8(event.phase)
        .u8(event.button)
        .u16(0)
        .i64(event.x32_32)
        .i64(event.y32_32),
    );
  }

  axis(
    view: YasSurfaceView,
    feedback: YasSurfaceFrameFeedback,
    event: {
      clientMonotonicNs: bigint;
      source: number;
      flags: number;
      dx32_32: bigint;
      dy32_32: bigint;
      stepsX: number;
      stepsY: number;
    },
  ): void {
    if (
      event.source > YAS_SURFACE_AXIS_SOURCE_WHEEL_TILT ||
      event.flags & ~YAS_SURFACE_AXIS_FLAGS_MASK
    )
      throw new YasProtocolError("invalid Surface AXIS event");
    this.withFeedback(view, feedback, YAS_SURFACE_AXIS, (writer) =>
      writer
        .u64(event.clientMonotonicNs)
        .u8(event.source)
        .u8(event.flags)
        .u16(0)
        .i64(event.dx32_32)
        .i64(event.dy32_32)
        .i32(event.stepsX)
        .i32(event.stepsY),
    );
  }

  touch(
    view: YasSurfaceView,
    clientMonotonicNs: bigint,
    phase: number,
    contacts: readonly YasSurfaceRemoteContact[],
  ): void {
    const mayBeEmpty =
      phase === YAS_SURFACE_TOUCH_PHASE_CANCEL ||
      phase === YAS_SURFACE_TOUCH_PHASE_FRAME;
    if (
      phase > YAS_SURFACE_TOUCH_PHASE_FRAME ||
      (!mayBeEmpty && contacts.length === 0) ||
      contacts.length > 0xffff
    )
      throw new YasProtocolError("invalid Surface TOUCH contact count");
    const ids = new Set<number>();
    const writer = new YasWriter()
      .u32(view.result.viewId)
      .u64(clientMonotonicNs)
      .u8(phase)
      .u8(0)
      .u16(contacts.length);
    for (const contact of contacts) {
      if (ids.has(contact.contactId))
        throw new YasProtocolError("duplicate Surface touch contact");
      ids.add(contact.contactId);
      writer.u32(contact.contactId).i64(contact.x32_32).i64(contact.y32_32);
    }
    this.connection.sendEvent(
      YAS_FAMILY_SURFACE,
      YAS_SURFACE_TOUCH,
      writer.finish(),
    );
  }

  hasView(viewId: number): boolean {
    return this.views.has(viewId);
  }

  removeView(viewId: number): void {
    this.views.delete(viewId);
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.endpointGeneration++;
    this.viewGeneration++;
    for (const remove of this.removeListeners) remove();
    this.removeListeners = [];
    for (const view of [...this.views.values()]) {
      this.closeViewRemote(view.result.viewId);
      view.closeLocal();
    }
    for (const handle of this.appEndpoints.keys())
      this.releaseAppEndpointRemote(handle);
    for (const handle of [...this.appEndpoints.keys()])
      this.tombstoneAppEndpoint(handle);
    this.appEndpointOperations.clear();
    this.remoteListeners.clear();
    this.catalog.dispose();
  }

  private closeViewRemote(viewId: number): void {
    try {
      void this.connection
        .request(
          YAS_FAMILY_SURFACE,
          YAS_SURFACE_CLOSE_VIEW,
          new YasWriter().u32(viewId).finish(),
        )
        .catch(() => undefined);
    } catch {
      // Family invalidation may make the cleanup request unavailable.
    }
  }

  private releaseAppEndpointRemote(appHandle: bigint): void {
    try {
      void this.connection
        .requestDecoded(
          YAS_FAMILY_SURFACE,
          YAS_SURFACE_RELEASE_APP_ENDPOINT,
          encodeSurfaceReleaseAppEndpoint({
            appHandle,
            operationId: surfaceOperationId(),
          }),
          (body) => {
            new YasCursor(body).end("Surface RELEASE_APP_ENDPOINT Result");
          },
        )
        .catch(() => undefined);
    } catch {
      // Family invalidation may make the cleanup request unavailable.
    }
  }

  private pruneExpiredAppEndpoints(): void {
    for (const [handle, endpoint] of this.appEndpoints)
      if (
        this.connection.nanosecondsUntilServerTime(endpoint.expiresServerNs) ===
        0n
      )
        this.tombstoneAppEndpoint(handle);
  }

  private tombstoneAppEndpoint(appHandle: bigint): void {
    const endpoint = this.appEndpoints.get(appHandle);
    if (!endpoint) return;
    this.appEndpoints.delete(appHandle);
    const operation = this.appEndpointOperations.get(endpoint.operationKey);
    if (operation?.result?.appHandle === appHandle) operation.result = null;
  }

  private withFeedback(
    view: YasSurfaceView,
    feedback: YasSurfaceFrameFeedback,
    kind: number,
    append: (writer: YasWriter) => YasWriter,
  ): void {
    if (!this.hasView(view.result.viewId))
      throw new YasProtocolError("Surface input names a closed view");
    view.recordFeedback(feedback);
    const writer = new YasWriter()
      .u32(view.result.viewId)
      .bytes(encodeSurfaceFrameFeedback(feedback));
    this.connection.sendEvent(
      YAS_FAMILY_SURFACE,
      kind,
      append(writer).finish(),
    );
  }

  private revisionRequest(kind: number, payload: Uint8Array): Promise<bigint> {
    return this.connection.requestDecoded(
      YAS_FAMILY_SURFACE,
      kind,
      payload,
      (body) => {
        const cursor = new YasCursor(body);
        const revision = cursor.u64("Surface state revision");
        cursor.end("Surface revision Result");
        if (revision === 0n)
          throw new YasProtocolError("Surface state revision is zero");
        return revision;
      },
    );
  }
}

function surfaceOperationId(): Uint8Array {
  const value = new Uint8Array(16);
  globalThis.crypto.getRandomValues(value);
  return value;
}

function validateSurfaceRecord(value: YasSurfaceRecord): void {
  requireHandle(value.surfaceHandle, "Surface handle");
  if (
    value.revision === 0n ||
    value.compositeWidth === 0 ||
    value.compositeHeight === 0 ||
    value.logicalWidth32_32 <= 0n ||
    value.logicalHeight32_32 <= 0n
  )
    throw new YasProtocolError("invalid Surface geometry or revision");
  surfaceActivationRevision(value);
  surfaceCursorState(value);
  surfaceTextInputState(value);
}

function validateSurfaceFrame(value: YasSurfaceFrame): void {
  if (
    value.viewId === 0 ||
    value.codecVersion === 0 ||
    value.fragmentCount === 0 ||
    value.fragmentIndex >= value.fragmentCount ||
    value.completeLength === 0 ||
    value.payload.length === 0 ||
    value.payload.length > value.completeLength ||
    value.flags & ~YAS_SURFACE_FRAME_FLAGS_MASK ||
    (value.codecVersion !== YAS_SURFACE_CODEC_H264_V1 &&
      value.codecVersion !== YAS_SURFACE_CODEC_AV1_V1 &&
      value.codecVersion !== YAS_SURFACE_CODEC_PNG_V1)
  )
    throw new YasProtocolError("invalid Surface FRAME fragment");
}

function validateSurfaceRemoteInput(value: YasSurfaceRemoteInput): void {
  requireHandle(value.surfaceHandle, "Surface remote-input surface handle");
  requireHandle(value.seatHandle, "Surface remote-input seat handle");
  const pointer = value.inputKind === YAS_SURFACE_REMOTE_INPUT_POINTER;
  if (
    (!pointer && value.inputKind !== YAS_SURFACE_REMOTE_INPUT_TOUCH) ||
    value.contacts.length > YAS_SURFACE_MAX_REMOTE_CONTACTS ||
    (pointer &&
      (value.contacts.length !== 1 || value.contacts[0]!.contactId !== 0))
  )
    throw new YasProtocolError("invalid Surface remote-input kind or contacts");
  const ids = new Set<number>();
  for (const contact of value.contacts) {
    if (ids.has(contact.contactId))
      throw new YasProtocolError("duplicate Surface remote contact");
    ids.add(contact.contactId);
  }
}

function validateCaptureTransfer(
  value: Extract<YasInlineOrTransfer, { delivery: "transfer" }>,
): void {
  if (
    value.descriptor.mode !== YAS_TRANSFER_MODE_BYTE ||
    !(value.descriptor.direction & YAS_TRANSFER_SENDER_TO_RECEIVER)
  )
    throw new YasProtocolError("invalid Surface CAPTURE Transfer");
}

function transferCapture(
  value: Extract<YasInlineOrTransfer, { delivery: "transfer" }>,
  transfer: YasTransfer,
): YasSurfaceCaptureResult {
  let collected: Promise<Uint8Array> | undefined;
  return {
    byteLength: value.byteLength,
    contentHash: value.contentHash,
    bytes: () => (collected ??= transfer.collect(value.byteLength)),
  };
}

function requireHandle(value: bigint, name: string): void {
  if (value === 0n) throw new YasProtocolError(`${name} is zero`);
}

function requireOperationId(value: Uint8Array, name: string): void {
  if (value.length !== 16)
    throw new YasProtocolError(`${name} operation ID is not 16 bytes`);
}

function mergeExtensions(
  previous: readonly YasExtension[],
  patch: readonly YasExtension[],
): YasExtension[] {
  const merged = new Map(
    previous.map((extension) => [extension.tag, extension]),
  );
  for (const extension of patch) merged.set(extension.tag, extension);
  return [...merged.values()].sort((left, right) => left.tag - right.tag);
}

function byteKey(bytes: Uint8Array): string {
  let value = "";
  for (const byte of bytes) value += String.fromCharCode(byte);
  return value;
}

function concat(chunks: readonly Uint8Array[], length: number): Uint8Array {
  const value = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    value.set(chunk, offset);
    offset += chunk.length;
  }
  return value;
}

function surfaceLimitU32(
  extensions: readonly YasExtension[],
  tag: number,
): number {
  const extension = extensions.find((value) => value.tag === tag);
  if (!extension) throw new YasProtocolError("missing Surface family limit");
  const cursor = new YasCursor(extension.value);
  const value = cursor.u32("Surface family limit");
  cursor.end("Surface family limit");
  return value;
}

function surfaceLimitU64(
  extensions: readonly YasExtension[],
  tag: number,
): bigint {
  const extension = extensions.find((value) => value.tag === tag);
  if (!extension) throw new YasProtocolError("missing Surface family limit");
  const cursor = new YasCursor(extension.value);
  const value = cursor.u64("Surface family limit");
  cursor.end("Surface family limit");
  return value;
}

function surfaceLimit32(tag: number, value: number): YasExtension {
  return { tag, value: new YasWriter().u32(value).finish() };
}

function surfaceLimit64(tag: number, value: bigint): YasExtension {
  return { tag, value: new YasWriter().u64(value).finish() };
}

function validateSurfaceLimits(value: YasSurfaceLimits): void {
  const valid = (candidate: number, maximum: number) =>
    Number.isInteger(candidate) && candidate > 0 && candidate <= maximum;
  if (
    !valid(value.maxSurfacesPerSession, YAS_SURFACE_MAX_SURFACES_PER_SESSION) ||
    !valid(value.maxViewsPerSession, YAS_SURFACE_MAX_VIEWS_PER_SESSION) ||
    !valid(value.maxViewDimension, YAS_SURFACE_MAX_VIEW_DIMENSION) ||
    value.maxViewPixels <= 0n ||
    value.maxViewPixels > BigInt(YAS_SURFACE_MAX_VIEW_PIXELS) ||
    !valid(value.maxFrameRate, YAS_SURFACE_MAX_FRAME_RATE) ||
    !valid(value.maxInlineCursorBytes, YAS_SURFACE_MAX_INLINE_CURSOR_BYTES) ||
    !valid(value.maxRemoteContacts, YAS_SURFACE_MAX_REMOTE_CONTACTS) ||
    !valid(
      value.maxAppEndpointsPerSession,
      YAS_SURFACE_MAX_APP_ENDPOINTS_PER_SESSION,
    ) ||
    value.maxAppEndpointLifetimeNs <= 0n ||
    value.maxAppEndpointLifetimeNs >
      BigInt(YAS_SURFACE_MAX_APP_ENDPOINT_LIFETIME_NS)
  )
    throw new YasProtocolError("invalid Surface family limits");
}
