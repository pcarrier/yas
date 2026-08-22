/** YAS Media family v1 codecs and browser client. */

import * as g from "./generated";
import type { YasConnection } from "./session";
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
  YasStateCatalogueRetention,
  YasStateSubscription,
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
  type YasTransfer,
  type YasTransferDescriptor,
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

const textEncoder = new TextEncoder();

export {
  YAS_FAMILY_MEDIA,
  YAS_MEDIA_ACQUIRE_DEVICE,
  YAS_MEDIA_ASSET_CONTENT_KIND,
  YAS_MEDIA_CLOSE_STREAM,
  YAS_MEDIA_FETCH_ASSET,
  YAS_MEDIA_FRAME,
  YAS_MEDIA_FRAME_ACK,
  YAS_MEDIA_OPEN_OUTPUT,
  YAS_MEDIA_PLAYER_ACTION,
  YAS_MEDIA_PORTAL_CLOSE,
  YAS_MEDIA_PORTAL_REPLY,
  YAS_MEDIA_PORTAL_REQUEST,
  YAS_MEDIA_RELEASE_DEVICE,
  YAS_MEDIA_STATE,
  YAS_MEDIA_STATE_ACK,
  YAS_MEDIA_STREAM_STATUS,
  YAS_MEDIA_UNWATCH,
  YAS_MEDIA_VERSION,
  YAS_MEDIA_WATCH,
} from "./generated";

export interface YasMediaFormat {
  codec: number;
  channels: number;
  sampleRate: number;
  width: number;
  height: number;
  frameRateMilli: number;
  extensions: readonly YasExtension[];
}

export interface YasMediaDeviceRecord {
  kind: "device";
  deviceHandle: bigint;
  revision: bigint;
  deviceKind: number;
  state: number;
  flags: number;
  name: string;
  formats: readonly YasMediaFormat[];
  extensions: readonly YasExtension[];
}

export interface YasMediaLeaseRecord {
  kind: "lease";
  leaseHandle: bigint;
  revision: bigint;
  deviceHandle: bigint;
  ownerSession: Uint8Array;
  lifecycle: number;
  expiresServerNs: bigint;
  extensions: readonly YasExtension[];
}

export interface YasMediaPortalRecord {
  kind: "portal";
  portalHandle: bigint;
  revision: bigint;
  portalKind: number;
  state: number;
  ownerSession: Uint8Array;
  metadata: YasMediaPortalRecordMetadata;
  extensions: readonly YasExtension[];
}

export interface YasMediaPlayerRecord {
  kind: "player";
  playerHandle: bigint;
  revision: bigint;
  state: number;
  flags: number;
  positionUs: bigint;
  durationUs: bigint;
  identity: string;
  title: string;
  artist: string;
  album: string;
  extensions: readonly YasExtension[];
}

export interface YasMediaSnapshot {
  revision: bigint;
  devices: readonly YasMediaDeviceRecord[];
  leases: readonly YasMediaLeaseRecord[];
  portals: readonly YasMediaPortalRecord[];
  players: readonly YasMediaPlayerRecord[];
}

export interface YasMediaOpenOutput {
  deviceHandle: bigint;
  formats: readonly YasMediaFormat[];
  latencyTargetNs: bigint;
  /** 0 selects the server default. */
  targetBitrateKbps: number;
  extensions?: readonly YasExtension[];
}

export interface YasMediaOpenOutputResult {
  streamHandle: bigint;
  selectedFormat: YasMediaFormat;
}

export interface YasMediaAcquireDevice {
  deviceHandle: bigint;
  operationId: Uint8Array;
  kind: number;
  leaseDurationNs: bigint;
  formats: readonly YasMediaFormat[];
  extensions?: readonly YasExtension[];
}

export interface YasMediaAcquireDeviceResult {
  leaseHandle: bigint;
  streamHandle: bigint;
  expiresServerNs: bigint;
  selectedFormat: YasMediaFormat;
}

export interface YasMediaPortalReply {
  portalHandle: bigint;
  revision: bigint;
  operationId: Uint8Array;
  kind: number;
  decision: number;
  metadata: YasMediaPortalReplyMetadata;
  extensions?: readonly YasExtension[];
}

export interface YasMediaPortalClose {
  portalHandle: bigint;
  revision: bigint;
  operationId: Uint8Array;
  extensions?: readonly YasExtension[];
}

export interface YasMediaPlayerAction {
  playerHandle: bigint;
  revision: bigint;
  operationId: Uint8Array;
  action: number;
  value: bigint;
  extensions?: readonly YasExtension[];
}

export interface YasMediaPortalRequest {
  portalHandle: bigint;
  revision: bigint;
  kind: number;
  flags: number;
  applicationHandle: bigint;
  metadata: YasMediaPortalRequestMetadata;
  extensions: readonly YasExtension[];
}

export interface YasMediaPortalChoiceValue {
  id: string;
  value: string;
}

export interface YasMediaPortalChoice {
  id: string;
  label: string;
  initial: string;
  options: readonly YasMediaPortalChoiceValue[];
}

export interface YasMediaPortalAccessRequestMetadata {
  kind: "access";
  deadlineServerNs: bigint;
  parentSurfaceHandle: bigint | null;
  appId: string;
  title: string;
  subtitle: string;
  body: string;
  denyLabel: string;
  grantLabel: string;
  iconName: string;
  choices: readonly YasMediaPortalChoice[];
}

export interface YasMediaScreenCastCandidate {
  surfaceHandle: bigint;
  width: number;
  height: number;
  title: string;
  appId: string;
  thumbnailHash: Uint8Array | null;
}

export interface YasMediaPortalScreenCastRequestMetadata {
  kind: "screencast";
  deadlineServerNs: bigint;
  parentSurfaceHandle: bigint | null;
  appId: string;
  multiple: boolean;
  candidates: readonly YasMediaScreenCastCandidate[];
}

export type YasMediaPortalRequestMetadata =
  | YasMediaPortalAccessRequestMetadata
  | YasMediaPortalScreenCastRequestMetadata;

export type YasMediaPortalReplyMetadata =
  | { kind: "empty" }
  | {
      kind: "accessGrant";
      choices: readonly YasMediaPortalChoiceValue[];
    }
  | {
      kind: "screencastGrant";
      surfaceHandles: readonly bigint[];
    };

export type YasMediaPortalGrantedMetadata =
  | {
      kind: "accessGranted";
      choices: readonly YasMediaPortalChoiceValue[];
    }
  | {
      kind: "screencastGranted";
      streams: readonly {
        surfaceHandle: bigint;
        streamHandle: bigint;
      }[];
    };

export type YasMediaPortalRecordMetadata =
  | { kind: "request"; request: YasMediaPortalRequestMetadata }
  | { kind: "grant"; grant: YasMediaPortalGrantedMetadata }
  | { kind: "empty" };

export interface YasMediaFrame {
  streamHandle: bigint;
  sequence: bigint;
  captureTime: bigint;
  presentationTime: bigint;
  codecVersion: number;
  flags: number;
  fragmentIndex: number;
  fragmentCount: number;
  completeLength: number;
  payload: Uint8Array;
}

export interface YasMediaFrameAck {
  streamHandle: bigint;
  consumedSequence: bigint;
  queueDepth: number;
  desiredCreditFrames: number;
}

export interface YasMediaStreamStatus {
  streamHandle: bigint;
  revision: bigint;
  status: number;
  flags: number;
  codecConfig: Uint8Array;
  extensions: readonly YasExtension[];
}

export interface YasMediaContent {
  byteLength: bigint;
  contentHash: Uint8Array;
  bytes(): Promise<Uint8Array>;
}

export function encodeMediaFormat(value: YasMediaFormat): Uint8Array {
  validateMediaFormat(value);
  return new YasWriter()
    .u16(value.codec)
    .u16(value.channels)
    .u32(value.sampleRate)
    .u32(value.width)
    .u32(value.height)
    .u32(value.frameRateMilli)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

function decodeMediaFormatFrom(cursor: YasCursor): YasMediaFormat {
  const value: YasMediaFormat = {
    codec: cursor.u16("Media codec"),
    channels: cursor.u16("Media channels"),
    sampleRate: cursor.u32("Media sample rate"),
    width: cursor.u32("Media width"),
    height: cursor.u32("Media height"),
    frameRateMilli: cursor.u32("Media frame rate"),
    extensions: decodeExtensions(cursor, new Set(), "Media format extensions"),
  };
  validateMediaFormat(value);
  return value;
}

export function decodeMediaFormat(bytes: Uint8Array): YasMediaFormat {
  const cursor = new YasCursor(bytes);
  const value = decodeMediaFormatFrom(cursor);
  cursor.end("Media format");
  return value;
}

function encodeFormats(
  writer: YasWriter,
  formats: readonly YasMediaFormat[],
): void {
  validateFormats(formats);
  writer.u16(formats.length).u16(0);
  for (const format of formats) writer.bytes(encodeMediaFormat(format));
}

function decodeFormats(cursor: YasCursor): YasMediaFormat[] {
  const count = cursor.u16("Media format count");
  if (
    cursor.u16("Media format reserved") !== 0 ||
    count === 0 ||
    count > g.YAS_MEDIA_MAX_FORMATS ||
    count > Math.floor(cursor.remaining / 24)
  )
    throw new YasProtocolError("invalid Media format count or reserved field");
  const formats: YasMediaFormat[] = [];
  for (let index = 0; index < count; index++)
    formats.push(decodeMediaFormatFrom(cursor));
  validateFormats(formats);
  return formats;
}

export function encodeMediaOpenOutput(value: YasMediaOpenOutput): Uint8Array {
  requireHandle(value.deviceHandle, "Media output device");
  validateFormats(value.formats, true);
  if (
    !Number.isInteger(value.targetBitrateKbps) ||
    value.targetBitrateKbps < 0 ||
    value.targetBitrateKbps > g.YAS_MEDIA_MAX_OUTPUT_BITRATE_KBPS
  )
    throw new YasProtocolError("invalid Media output target bitrate");
  const writer = new YasWriter().u64(value.deviceHandle);
  encodeFormats(writer, value.formats);
  return writer
    .u64(value.latencyTargetNs)
    .u16(value.targetBitrateKbps)
    .u16(0)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeMediaOpenOutput(bytes: Uint8Array): YasMediaOpenOutput {
  const cursor = new YasCursor(bytes);
  const deviceHandle = cursor.u64("Media output device");
  const formats = decodeFormats(cursor);
  const latencyTargetNs = cursor.u64("Media latency target");
  const targetBitrateKbps = cursor.u16("Media output target bitrate");
  if (cursor.u16("Media OPEN_OUTPUT reserved") !== 0)
    throw new YasProtocolError("Media OPEN_OUTPUT reserved field is nonzero");
  const value = {
    deviceHandle,
    formats,
    latencyTargetNs,
    targetBitrateKbps,
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Media OPEN_OUTPUT extensions",
    ),
  };
  cursor.end("Media OPEN_OUTPUT");
  encodeMediaOpenOutput(value);
  return value;
}

export function encodeMediaOpenOutputResult(
  value: YasMediaOpenOutputResult,
): Uint8Array {
  requireHandle(value.streamHandle, "Media stream");
  if (!isAudioCodec(value.selectedFormat.codec))
    throw new YasProtocolError("Media output selected a non-audio format");
  return new YasWriter()
    .u64(value.streamHandle)
    .bytes(encodeMediaFormat(value.selectedFormat))
    .finish();
}

export function decodeMediaOpenOutputResult(
  bytes: Uint8Array,
): YasMediaOpenOutputResult {
  const cursor = new YasCursor(bytes);
  const value = {
    streamHandle: cursor.u64("Media stream"),
    selectedFormat: decodeMediaFormatFrom(cursor),
  };
  cursor.end("Media OPEN_OUTPUT Result");
  encodeMediaOpenOutputResult(value);
  return value;
}

export function encodeMediaAcquireDevice(
  value: YasMediaAcquireDevice,
): Uint8Array {
  requireHandle(value.deviceHandle, "Media device");
  requireOperationId(value.operationId, "Media ACQUIRE_DEVICE");
  if (
    value.kind !== g.YAS_MEDIA_KIND_MICROPHONE &&
    value.kind !== g.YAS_MEDIA_KIND_CAMERA
  )
    throw new YasProtocolError("invalid Media acquired device kind");
  validateFormats(value.formats, value.kind === g.YAS_MEDIA_KIND_MICROPHONE);
  const writer = new YasWriter()
    .u64(value.deviceHandle)
    .bytes(value.operationId)
    .u8(value.kind)
    .bytes(new Uint8Array(3))
    .u64(value.leaseDurationNs);
  encodeFormats(writer, value.formats);
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeMediaAcquireDevice(
  bytes: Uint8Array,
): YasMediaAcquireDevice {
  const cursor = new YasCursor(bytes);
  const deviceHandle = cursor.u64("Media device");
  const operationId = new Uint8Array(cursor.take(16, "Media operation ID"));
  const kind = cursor.u8("Media device kind");
  requireZero(
    cursor.take(3, "Media ACQUIRE_DEVICE reserved"),
    "Media ACQUIRE_DEVICE",
  );
  const leaseDurationNs = cursor.u64("Media lease duration");
  const formats = decodeFormats(cursor);
  const value = {
    deviceHandle,
    operationId,
    kind,
    leaseDurationNs,
    formats,
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Media ACQUIRE_DEVICE extensions",
    ),
  };
  cursor.end("Media ACQUIRE_DEVICE");
  encodeMediaAcquireDevice(value);
  return value;
}

export function encodeMediaAcquireDeviceResult(
  value: YasMediaAcquireDeviceResult,
): Uint8Array {
  requireHandle(value.leaseHandle, "Media lease");
  requireHandle(value.streamHandle, "Media stream");
  return new YasWriter()
    .u64(value.leaseHandle)
    .u64(value.streamHandle)
    .u64(value.expiresServerNs)
    .bytes(encodeMediaFormat(value.selectedFormat))
    .finish();
}

export function decodeMediaAcquireDeviceResult(
  bytes: Uint8Array,
): YasMediaAcquireDeviceResult {
  const cursor = new YasCursor(bytes);
  const value = {
    leaseHandle: cursor.u64("Media lease"),
    streamHandle: cursor.u64("Media stream"),
    expiresServerNs: cursor.u64("Media lease expiry"),
    selectedFormat: decodeMediaFormatFrom(cursor),
  };
  cursor.end("Media ACQUIRE_DEVICE Result");
  encodeMediaAcquireDeviceResult(value);
  return value;
}

export function encodeMediaHandleOperation(
  handle: bigint,
  operationId: Uint8Array,
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  requireHandle(handle, "Media operation handle");
  requireOperationId(operationId, "Media handle operation");
  return new YasWriter()
    .u64(handle)
    .bytes(operationId)
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function decodeMediaHandleOperation(bytes: Uint8Array): {
  handle: bigint;
  operationId: Uint8Array;
  extensions: readonly YasExtension[];
} {
  const cursor = new YasCursor(bytes);
  const value = {
    handle: cursor.u64("Media operation handle"),
    operationId: new Uint8Array(cursor.take(16, "Media operation ID")),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Media operation extensions",
    ),
  };
  cursor.end("Media handle operation");
  encodeMediaHandleOperation(value.handle, value.operationId, value.extensions);
  return value;
}

export function encodeMediaPortalChoiceValue(
  value: YasMediaPortalChoiceValue,
): Uint8Array {
  validatePortalText(value.id, g.YAS_MEDIA_MAX_PORTAL_STRING_BYTES, true);
  validatePortalText(value.value, g.YAS_MEDIA_MAX_PORTAL_STRING_BYTES, true);
  return new YasWriter().utf8U16(value.id).utf8U16(value.value).finish();
}

export function decodeMediaPortalChoiceValue(
  bytes: Uint8Array,
): YasMediaPortalChoiceValue {
  const cursor = new YasCursor(bytes);
  const value = {
    id: cursor.utf8U16("Media portal choice ID"),
    value: cursor.utf8U16("Media portal choice value"),
  };
  cursor.end("Media portal choice value");
  encodeMediaPortalChoiceValue(value);
  return value;
}

export function encodeMediaPortalChoice(
  value: YasMediaPortalChoice,
): Uint8Array {
  validatePortalText(value.id, g.YAS_MEDIA_MAX_PORTAL_STRING_BYTES, true);
  validatePortalText(value.label, g.YAS_MEDIA_MAX_PORTAL_STRING_BYTES, true);
  validatePortalText(value.initial, g.YAS_MEDIA_MAX_PORTAL_STRING_BYTES, false);
  if (
    value.options.length === 0 ||
    value.options.length > g.YAS_MEDIA_MAX_PORTAL_CHOICE_OPTIONS
  )
    throw new YasProtocolError("invalid Media portal choice option count");
  const ids = new Set<string>();
  const encodedOptions = value.options.map((option) => {
    if (ids.has(option.id))
      throw new YasProtocolError("duplicate Media portal choice option");
    ids.add(option.id);
    return encodeMediaPortalChoiceValue(option);
  });
  if (value.initial.length !== 0 && !ids.has(value.initial))
    throw new YasProtocolError("invalid Media portal initial choice");
  const writer = new YasWriter()
    .utf8U16(value.id)
    .utf8U16(value.label)
    .utf8U16(value.initial)
    .u16(encodedOptions.length)
    .u16(0);
  for (const option of encodedOptions) writer.bytesU32(option);
  return writer.finish();
}

export function decodeMediaPortalChoice(
  bytes: Uint8Array,
): YasMediaPortalChoice {
  const cursor = new YasCursor(bytes);
  const id = cursor.utf8U16("Media portal choice ID");
  const label = cursor.utf8U16("Media portal choice label");
  const initial = cursor.utf8U16("Media portal initial choice");
  const count = cursor.u16("Media portal choice option count");
  if (
    cursor.u16("Media portal choice reserved") !== 0 ||
    count === 0 ||
    count > g.YAS_MEDIA_MAX_PORTAL_CHOICE_OPTIONS ||
    count > Math.floor(cursor.remaining / 4)
  )
    throw new YasProtocolError("invalid Media portal choice option count");
  const options: YasMediaPortalChoiceValue[] = [];
  for (let index = 0; index < count; index++)
    options.push(
      decodeMediaPortalChoiceValue(
        cursor.bytesU32("Media portal choice option"),
      ),
    );
  cursor.end("Media portal choice");
  const value = { id, label, initial, options };
  encodeMediaPortalChoice(value);
  return value;
}

export function encodeMediaPortalRequestMetadata(
  value: YasMediaPortalRequestMetadata,
): Uint8Array {
  if (value.deadlineServerNs === 0n)
    throw new YasProtocolError("Media portal deadline is zero");
  if (value.parentSurfaceHandle !== null)
    requireHandle(value.parentSurfaceHandle, "Media portal parent Surface");
  validatePortalText(value.appId, g.YAS_MEDIA_MAX_PORTAL_STRING_BYTES, true);
  if (value.kind === "access") {
    for (const [text, maximum, required] of [
      [value.title, g.YAS_MEDIA_MAX_PORTAL_STRING_BYTES, true],
      [value.subtitle, g.YAS_MEDIA_MAX_PORTAL_STRING_BYTES, false],
      [value.body, g.YAS_MEDIA_MAX_PORTAL_BODY_BYTES, false],
      [value.denyLabel, g.YAS_MEDIA_MAX_PORTAL_STRING_BYTES, true],
      [value.grantLabel, g.YAS_MEDIA_MAX_PORTAL_STRING_BYTES, true],
      [value.iconName, g.YAS_MEDIA_MAX_PORTAL_STRING_BYTES, false],
    ] as const)
      validatePortalText(text, maximum, required);
    if (value.choices.length > g.YAS_MEDIA_MAX_PORTAL_CHOICES)
      throw new YasProtocolError("too many Media portal choices");
    const ids = new Set<string>();
    const choices = value.choices.map((choice) => {
      if (ids.has(choice.id))
        throw new YasProtocolError("duplicate Media portal choice");
      ids.add(choice.id);
      return encodeMediaPortalChoice(choice);
    });
    const writer = new YasWriter()
      .u64(value.deadlineServerNs)
      .u64(value.parentSurfaceHandle ?? 0n)
      .utf8U16(value.appId)
      .utf8U16(value.title)
      .utf8U16(value.subtitle)
      .utf8U32(value.body)
      .utf8U16(value.denyLabel)
      .utf8U16(value.grantLabel)
      .utf8U16(value.iconName)
      .u16(choices.length)
      .u16(0);
    for (const choice of choices) writer.bytesU32(choice);
    return writer.finish();
  }
  if (
    value.candidates.length === 0 ||
    value.candidates.length > g.YAS_MEDIA_MAX_SCREENCAST_CANDIDATES
  )
    throw new YasProtocolError("invalid Media screencast candidate count");
  const handles = new Set<bigint>();
  const candidates = value.candidates.map((candidate) => {
    if (handles.has(candidate.surfaceHandle))
      throw new YasProtocolError("duplicate Media screencast candidate");
    handles.add(candidate.surfaceHandle);
    return encodeMediaScreenCastCandidate(candidate);
  });
  const writer = new YasWriter()
    .u64(value.deadlineServerNs)
    .u64(value.parentSurfaceHandle ?? 0n)
    .utf8U16(value.appId)
    .u8(value.multiple ? 1 : 0)
    .bytes(new Uint8Array(3))
    .u16(candidates.length)
    .u16(0);
  for (const candidate of candidates) writer.bytesU32(candidate);
  return writer.finish();
}

export function decodeMediaPortalRequestMetadata(
  kind: number,
  bytes: Uint8Array,
): YasMediaPortalRequestMetadata {
  const cursor = new YasCursor(bytes);
  const deadlineServerNs = cursor.u64("Media portal deadline");
  const encodedParent = cursor.u64("Media portal parent Surface");
  const parentSurfaceHandle = encodedParent === 0n ? null : encodedParent;
  const appId = cursor.utf8U16("Media portal application ID");
  let value: YasMediaPortalRequestMetadata;
  if (kind === g.YAS_MEDIA_PORTAL_KIND_ACCESS) {
    const title = cursor.utf8U16("Media access portal title");
    const subtitle = cursor.utf8U16("Media access portal subtitle");
    const body = cursor.utf8U32("Media access portal body");
    const denyLabel = cursor.utf8U16("Media access portal deny label");
    const grantLabel = cursor.utf8U16("Media access portal grant label");
    const iconName = cursor.utf8U16("Media access portal icon name");
    const count = cursor.u16("Media access portal choice count");
    if (
      cursor.u16("Media access portal reserved") !== 0 ||
      count > g.YAS_MEDIA_MAX_PORTAL_CHOICES ||
      count > Math.floor(cursor.remaining / 4)
    )
      throw new YasProtocolError("invalid Media access portal choice count");
    const choices: YasMediaPortalChoice[] = [];
    for (let index = 0; index < count; index++)
      choices.push(
        decodeMediaPortalChoice(cursor.bytesU32("Media portal choice")),
      );
    value = {
      kind: "access",
      deadlineServerNs,
      parentSurfaceHandle,
      appId,
      title,
      subtitle,
      body,
      denyLabel,
      grantLabel,
      iconName,
      choices,
    };
  } else if (kind === g.YAS_MEDIA_PORTAL_KIND_SCREENCAST) {
    const multiple = cursor.u8("Media screencast multiple");
    if (multiple > 1)
      throw new YasProtocolError("invalid Media screencast multiple value");
    requireZero(
      cursor.take(3, "Media screencast reserved"),
      "Media screencast",
    );
    const count = cursor.u16("Media screencast candidate count");
    if (
      cursor.u16("Media screencast candidate reserved") !== 0 ||
      count === 0 ||
      count > g.YAS_MEDIA_MAX_SCREENCAST_CANDIDATES ||
      count > Math.floor(cursor.remaining / 4)
    )
      throw new YasProtocolError("invalid Media screencast candidate count");
    const candidates: YasMediaScreenCastCandidate[] = [];
    for (let index = 0; index < count; index++)
      candidates.push(
        decodeMediaScreenCastCandidate(
          cursor.bytesU32("Media screencast candidate"),
        ),
      );
    value = {
      kind: "screencast",
      deadlineServerNs,
      parentSurfaceHandle,
      appId,
      multiple: multiple !== 0,
      candidates,
    };
  } else {
    throw new YasProtocolError("invalid Media portal request kind");
  }
  cursor.end("Media portal request metadata");
  encodeMediaPortalRequestMetadata(value);
  return value;
}

export function encodeMediaScreenCastCandidate(
  value: YasMediaScreenCastCandidate,
): Uint8Array {
  requireHandle(value.surfaceHandle, "Media screencast candidate Surface");
  if (value.width === 0 || value.height === 0)
    throw new YasProtocolError("invalid Media screencast candidate dimensions");
  validatePortalText(value.title, g.YAS_MEDIA_MAX_PORTAL_STRING_BYTES, true);
  validatePortalText(value.appId, g.YAS_MEDIA_MAX_PORTAL_STRING_BYTES, true);
  if (value.thumbnailHash !== null)
    requireHash(value.thumbnailHash, "Media screencast thumbnail hash");
  const writer = new YasWriter()
    .u64(value.surfaceHandle)
    .u32(value.width)
    .u32(value.height)
    .utf8U16(value.title)
    .utf8U16(value.appId)
    .u8(value.thumbnailHash === null ? 0 : 1)
    .bytes(new Uint8Array(3));
  if (value.thumbnailHash !== null) writer.bytes(value.thumbnailHash);
  return writer.finish();
}

export function decodeMediaScreenCastCandidate(
  bytes: Uint8Array,
): YasMediaScreenCastCandidate {
  const cursor = new YasCursor(bytes);
  const surfaceHandle = cursor.u64("Media screencast candidate Surface");
  const width = cursor.u32("Media screencast candidate width");
  const height = cursor.u32("Media screencast candidate height");
  const title = cursor.utf8U16("Media screencast candidate title");
  const appId = cursor.utf8U16("Media screencast candidate application ID");
  const present = cursor.u8("Media screencast thumbnail hash presence");
  if (present > 1)
    throw new YasProtocolError("invalid Media thumbnail hash presence");
  requireZero(
    cursor.take(3, "Media screencast thumbnail reserved"),
    "Media screencast thumbnail",
  );
  const value = {
    surfaceHandle,
    width,
    height,
    title,
    appId,
    thumbnailHash:
      present === 0
        ? null
        : new Uint8Array(cursor.take(32, "Media thumbnail hash")),
  };
  cursor.end("Media screencast candidate");
  encodeMediaScreenCastCandidate(value);
  return value;
}

export function encodeMediaPortalReplyMetadata(
  kind: number,
  decision: number,
  value: YasMediaPortalReplyMetadata,
): Uint8Array {
  if (decision !== g.YAS_MEDIA_PORTAL_DECISION_GRANT) {
    if (value.kind !== "empty")
      throw new YasProtocolError("nonempty denied Media portal metadata");
    return new Uint8Array();
  }
  if (kind === g.YAS_MEDIA_PORTAL_KIND_ACCESS && value.kind === "accessGrant") {
    if (value.choices.length > g.YAS_MEDIA_MAX_PORTAL_CHOICES)
      throw new YasProtocolError("too many Media portal grant choices");
    const ids = new Set<string>();
    const choices = value.choices.map((choice) => {
      if (ids.has(choice.id))
        throw new YasProtocolError("duplicate Media portal grant choice");
      ids.add(choice.id);
      return encodeMediaPortalChoiceValue(choice);
    });
    const writer = new YasWriter().u16(choices.length).u16(0);
    for (const choice of choices) writer.bytesU32(choice);
    return writer.finish();
  }
  if (
    kind === g.YAS_MEDIA_PORTAL_KIND_SCREENCAST &&
    value.kind === "screencastGrant"
  ) {
    if (
      value.surfaceHandles.length === 0 ||
      value.surfaceHandles.length > g.YAS_MEDIA_MAX_SCREENCAST_CANDIDATES
    )
      throw new YasProtocolError("invalid Media screencast grant count");
    const handles = new Set<bigint>();
    const writer = new YasWriter().u16(value.surfaceHandles.length).u16(0);
    for (const handle of value.surfaceHandles) {
      requireHandle(handle, "granted Media Surface");
      if (handles.has(handle))
        throw new YasProtocolError("duplicate granted Media Surface");
      handles.add(handle);
      writer.u64(handle);
    }
    return writer.finish();
  }
  throw new YasProtocolError("invalid Media portal reply metadata");
}

export function decodeMediaPortalReplyMetadata(
  kind: number,
  decision: number,
  bytes: Uint8Array,
): YasMediaPortalReplyMetadata {
  if (decision !== g.YAS_MEDIA_PORTAL_DECISION_GRANT) {
    if (bytes.length !== 0)
      throw new YasProtocolError("nonempty denied Media portal metadata");
    return { kind: "empty" };
  }
  const cursor = new YasCursor(bytes);
  if (kind === g.YAS_MEDIA_PORTAL_KIND_ACCESS) {
    const count = cursor.u16("Media access grant choice count");
    if (
      cursor.u16("Media access grant reserved") !== 0 ||
      count > g.YAS_MEDIA_MAX_PORTAL_CHOICES ||
      count > Math.floor(cursor.remaining / 4)
    )
      throw new YasProtocolError("invalid Media access grant choice count");
    const choices: YasMediaPortalChoiceValue[] = [];
    for (let index = 0; index < count; index++)
      choices.push(
        decodeMediaPortalChoiceValue(
          cursor.bytesU32("Media access grant choice"),
        ),
      );
    cursor.end("Media access grant metadata");
    const value = { kind: "accessGrant" as const, choices };
    encodeMediaPortalReplyMetadata(kind, decision, value);
    return value;
  }
  if (kind === g.YAS_MEDIA_PORTAL_KIND_SCREENCAST) {
    const count = cursor.u16("Media screencast grant count");
    if (
      cursor.u16("Media screencast grant reserved") !== 0 ||
      count === 0 ||
      count > g.YAS_MEDIA_MAX_SCREENCAST_CANDIDATES ||
      count > Math.floor(cursor.remaining / 8)
    )
      throw new YasProtocolError("invalid Media screencast grant count");
    const surfaceHandles: bigint[] = [];
    for (let index = 0; index < count; index++)
      surfaceHandles.push(cursor.u64("granted Media Surface"));
    cursor.end("Media screencast grant metadata");
    const value = { kind: "screencastGrant" as const, surfaceHandles };
    encodeMediaPortalReplyMetadata(kind, decision, value);
    return value;
  }
  throw new YasProtocolError("invalid Media portal reply kind");
}

export function encodeMediaPortalReply(value: YasMediaPortalReply): Uint8Array {
  requireHandle(value.portalHandle, "Media portal");
  requireRevision(value.revision, "Media portal revision");
  requireOperationId(value.operationId, "Media PORTAL_REPLY");
  if (value.operationId.every((byte) => byte === 0))
    throw new YasProtocolError("Media PORTAL_REPLY operation ID is zero");
  if (value.kind > g.YAS_MEDIA_PORTAL_KIND_SCREENCAST)
    throw new YasProtocolError("invalid Media portal reply kind");
  const metadata = encodeMediaPortalReplyMetadata(
    value.kind,
    value.decision,
    value.metadata,
  );
  if (
    value.decision > g.YAS_MEDIA_PORTAL_DECISION_CANCEL ||
    metadata.length > g.YAS_MEDIA_MAX_PORTAL_METADATA_BYTES
  )
    throw new YasProtocolError("invalid Media portal decision or metadata");
  return new YasWriter()
    .u64(value.portalHandle)
    .u64(value.revision)
    .bytes(value.operationId)
    .u16(value.kind)
    .u8(value.decision)
    .u8(0)
    .bytesU32(metadata)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeMediaPortalReply(bytes: Uint8Array): YasMediaPortalReply {
  const cursor = new YasCursor(bytes);
  const portalHandle = cursor.u64("Media portal");
  const revision = cursor.u64("Media portal revision");
  const operationId = new Uint8Array(cursor.take(16, "Media operation ID"));
  const kind = cursor.u16("Media portal kind");
  const decision = cursor.u8("Media portal decision");
  if (cursor.u8("Media PORTAL_REPLY reserved") !== 0)
    throw new YasProtocolError("Media PORTAL_REPLY reserved is nonzero");
  const metadataBytes = cursor.bytesU32("Media portal metadata");
  const value = {
    portalHandle,
    revision,
    operationId,
    kind,
    decision,
    metadata: decodeMediaPortalReplyMetadata(kind, decision, metadataBytes),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Media PORTAL_REPLY extensions",
    ),
  };
  cursor.end("Media PORTAL_REPLY");
  encodeMediaPortalReply(value);
  return value;
}

export function encodeMediaPortalClose(value: YasMediaPortalClose): Uint8Array {
  requireHandle(value.portalHandle, "Media portal");
  requireRevision(value.revision, "Media portal revision");
  requireOperationId(value.operationId, "Media PORTAL_CLOSE");
  if (value.operationId.every((byte) => byte === 0))
    throw new YasProtocolError("Media PORTAL_CLOSE operation ID is zero");
  return new YasWriter()
    .u64(value.portalHandle)
    .u64(value.revision)
    .bytes(value.operationId)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeMediaPortalClose(bytes: Uint8Array): YasMediaPortalClose {
  const cursor = new YasCursor(bytes);
  const value = {
    portalHandle: cursor.u64("Media portal"),
    revision: cursor.u64("Media portal revision"),
    operationId: new Uint8Array(
      cursor.take(16, "Media PORTAL_CLOSE operation ID"),
    ),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Media PORTAL_CLOSE extensions",
    ),
  };
  cursor.end("Media PORTAL_CLOSE");
  encodeMediaPortalClose(value);
  return value;
}

export function encodeMediaPlayerAction(
  value: YasMediaPlayerAction,
): Uint8Array {
  requireHandle(value.playerHandle, "Media player");
  requireRevision(value.revision, "Media player revision");
  requireOperationId(value.operationId, "Media PLAYER_ACTION");
  if (value.action > g.YAS_MEDIA_PLAYER_ACTION_RAISE)
    throw new YasProtocolError("invalid Media player action");
  return new YasWriter()
    .u64(value.playerHandle)
    .u64(value.revision)
    .bytes(value.operationId)
    .u16(value.action)
    .u16(0)
    .i64(value.value)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeMediaPlayerAction(
  bytes: Uint8Array,
): YasMediaPlayerAction {
  const cursor = new YasCursor(bytes);
  const playerHandle = cursor.u64("Media player");
  const revision = cursor.u64("Media player revision");
  const operationId = new Uint8Array(cursor.take(16, "Media operation ID"));
  const action = cursor.u16("Media player action");
  if (cursor.u16("Media PLAYER_ACTION reserved") !== 0)
    throw new YasProtocolError("Media PLAYER_ACTION reserved field is nonzero");
  const value = {
    playerHandle,
    revision,
    operationId,
    action,
    value: cursor.i64("Media player action value"),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Media PLAYER_ACTION extensions",
    ),
  };
  cursor.end("Media PLAYER_ACTION");
  encodeMediaPlayerAction(value);
  return value;
}

export function encodeMediaFetchAsset(
  contentHash: Uint8Array,
  initialReceiveCredit: bigint,
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  requireHash(contentHash, "Media asset hash");
  return new YasWriter()
    .bytes(contentHash)
    .u64(initialReceiveCredit)
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function decodeMediaFetchAsset(bytes: Uint8Array): {
  contentHash: Uint8Array;
  initialReceiveCredit: bigint;
  extensions: readonly YasExtension[];
} {
  const cursor = new YasCursor(bytes);
  const value = {
    contentHash: new Uint8Array(cursor.take(32, "Media asset hash")),
    initialReceiveCredit: cursor.u64("Media asset receive credit"),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Media FETCH_ASSET extensions",
    ),
  };
  cursor.end("Media FETCH_ASSET");
  encodeMediaFetchAsset(
    value.contentHash,
    value.initialReceiveCredit,
    value.extensions,
  );
  return value;
}

export function encodeMediaPortalRequest(
  value: YasMediaPortalRequest,
): Uint8Array {
  requireHandle(value.portalHandle, "Media portal");
  requireRevision(value.revision, "Media portal revision");
  const metadata = encodeMediaPortalRequestMetadata(value.metadata);
  if (
    value.kind > g.YAS_MEDIA_PORTAL_KIND_SCREENCAST ||
    value.flags & ~g.YAS_MEDIA_PORTAL_REQUEST_FLAGS ||
    (value.kind === g.YAS_MEDIA_PORTAL_KIND_ACCESS) !==
      (value.metadata.kind === "access") ||
    metadata.length > g.YAS_MEDIA_MAX_PORTAL_METADATA_BYTES
  )
    throw new YasProtocolError("invalid Media portal request");
  return new YasWriter()
    .u64(value.portalHandle)
    .u64(value.revision)
    .u16(value.kind)
    .u16(value.flags)
    .u64(value.applicationHandle)
    .bytesU32(metadata)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeMediaPortalRequest(
  bytes: Uint8Array,
): YasMediaPortalRequest {
  const cursor = new YasCursor(bytes);
  const portalHandle = cursor.u64("Media portal");
  const revision = cursor.u64("Media portal revision");
  const kind = cursor.u16("Media portal kind");
  const flags = cursor.u16("Media portal flags");
  const applicationHandle = cursor.u64("Media application handle");
  const value = {
    portalHandle,
    revision,
    kind,
    flags,
    applicationHandle,
    metadata: decodeMediaPortalRequestMetadata(
      kind,
      cursor.bytesU32("Media portal metadata"),
    ),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Media PORTAL_REQUEST extensions",
    ),
  };
  cursor.end("Media PORTAL_REQUEST");
  encodeMediaPortalRequest(value);
  return value;
}

export function encodeMediaFrame(value: YasMediaFrame): Uint8Array {
  validateMediaFrame(value);
  return new YasWriter()
    .u64(value.streamHandle)
    .u64(value.sequence)
    .u64(value.captureTime)
    .u64(value.presentationTime)
    .u16(value.codecVersion)
    .u16(value.flags)
    .u16(value.fragmentIndex)
    .u16(value.fragmentCount)
    .u32(value.completeLength)
    .bytes(value.payload)
    .finish();
}

export function decodeMediaFrame(bytes: Uint8Array): YasMediaFrame {
  const cursor = new YasCursor(bytes);
  const value = {
    streamHandle: cursor.u64("Media stream"),
    sequence: cursor.u64("Media frame sequence"),
    captureTime: cursor.u64("Media capture time"),
    presentationTime: cursor.u64("Media presentation time"),
    codecVersion: cursor.u16("Media frame codec"),
    flags: cursor.u16("Media frame flags"),
    fragmentIndex: cursor.u16("Media fragment index"),
    fragmentCount: cursor.u16("Media fragment count"),
    completeLength: cursor.u32("Media complete length"),
    payload: new Uint8Array(
      cursor.take(cursor.remaining, "Media frame payload"),
    ),
  };
  cursor.end("Media FRAME");
  validateMediaFrame(value);
  return value;
}

export function encodeMediaFrameAck(value: YasMediaFrameAck): Uint8Array {
  requireHandle(value.streamHandle, "Media stream");
  return new YasWriter()
    .u64(value.streamHandle)
    .u64(value.consumedSequence)
    .u16(value.queueDepth)
    .u16(value.desiredCreditFrames)
    .finish();
}

export function decodeMediaFrameAck(bytes: Uint8Array): YasMediaFrameAck {
  const cursor = new YasCursor(bytes);
  const value = {
    streamHandle: cursor.u64("Media stream"),
    consumedSequence: cursor.u64("Media consumed sequence"),
    queueDepth: cursor.u16("Media queue depth"),
    desiredCreditFrames: cursor.u16("Media desired frame credit"),
  };
  cursor.end("Media FRAME_ACK");
  encodeMediaFrameAck(value);
  return value;
}

export function encodeMediaStreamStatus(
  value: YasMediaStreamStatus,
): Uint8Array {
  requireHandle(value.streamHandle, "Media stream");
  requireRevision(value.revision, "Media stream revision");
  if (
    value.status > g.YAS_MEDIA_STREAM_ERROR ||
    value.flags & ~g.YAS_MEDIA_STREAM_FLAGS_MASK ||
    value.codecConfig.length > g.YAS_MEDIA_MAX_INLINE_METADATA_BYTES
  )
    throw new YasProtocolError("invalid Media stream status");
  return new YasWriter()
    .u64(value.streamHandle)
    .u64(value.revision)
    .u16(value.status)
    .u16(value.flags)
    .bytesU32(value.codecConfig)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeMediaStreamStatus(
  bytes: Uint8Array,
): YasMediaStreamStatus {
  const cursor = new YasCursor(bytes);
  const value = {
    streamHandle: cursor.u64("Media stream"),
    revision: cursor.u64("Media stream revision"),
    status: cursor.u16("Media stream status"),
    flags: cursor.u16("Media stream flags"),
    codecConfig: new Uint8Array(cursor.bytesU32("Media codec configuration")),
    extensions: decodeExtensions(cursor, new Set(), "Media stream extensions"),
  };
  cursor.end("Media STREAM_STATUS");
  encodeMediaStreamStatus(value);
  return value;
}

export function encodeMediaDeviceRecord(
  value: YasMediaDeviceRecord,
): Uint8Array {
  validateDevice(value);
  const writer = new YasWriter()
    .u64(value.deviceHandle)
    .u64(value.revision)
    .u8(value.deviceKind)
    .u8(value.state)
    .u16(value.flags)
    .utf8U16(value.name);
  encodeFormats(writer, value.formats);
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeMediaDeviceRecord(
  bytes: Uint8Array,
): YasMediaDeviceRecord {
  const cursor = new YasCursor(bytes);
  const deviceHandle = cursor.u64("Media device");
  const revision = cursor.u64("Media device revision");
  const deviceKind = cursor.u8("Media device kind");
  const state = cursor.u8("Media device state");
  const flags = cursor.u16("Media device flags");
  const name = cursor.utf8U16("Media device name");
  const formats = decodeFormats(cursor);
  const value = {
    kind: "device" as const,
    deviceHandle,
    revision,
    deviceKind,
    state,
    flags,
    name,
    formats,
    extensions: decodeExtensions(cursor, new Set(), "Media device extensions"),
  };
  cursor.end("Media device record");
  validateDevice(value);
  return value;
}

export function encodeMediaLeaseRecord(value: YasMediaLeaseRecord): Uint8Array {
  validateLease(value);
  return new YasWriter()
    .u64(value.leaseHandle)
    .u64(value.revision)
    .u64(value.deviceHandle)
    .bytes(value.ownerSession)
    .u8(value.lifecycle)
    .bytes(new Uint8Array(7))
    .u64(value.expiresServerNs)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeMediaLeaseRecord(bytes: Uint8Array): YasMediaLeaseRecord {
  const cursor = new YasCursor(bytes);
  const leaseHandle = cursor.u64("Media lease");
  const revision = cursor.u64("Media lease revision");
  const deviceHandle = cursor.u64("Media device");
  const ownerSession = new Uint8Array(cursor.take(16, "Media lease owner"));
  const lifecycle = cursor.u8("Media lease lifecycle");
  requireZero(cursor.take(7, "Media lease reserved"), "Media lease");
  const value = {
    kind: "lease" as const,
    leaseHandle,
    revision,
    deviceHandle,
    ownerSession,
    lifecycle,
    expiresServerNs: cursor.u64("Media lease expiry"),
    extensions: decodeExtensions(cursor, new Set(), "Media lease extensions"),
  };
  cursor.end("Media lease record");
  validateLease(value);
  return value;
}

export function encodeMediaPortalGrantedMetadata(
  portalKind: number,
  value: YasMediaPortalGrantedMetadata,
): Uint8Array {
  if (
    portalKind === g.YAS_MEDIA_PORTAL_KIND_ACCESS &&
    value.kind === "accessGranted"
  )
    return encodeMediaPortalReplyMetadata(
      portalKind,
      g.YAS_MEDIA_PORTAL_DECISION_GRANT,
      { kind: "accessGrant", choices: value.choices },
    );
  if (
    portalKind === g.YAS_MEDIA_PORTAL_KIND_SCREENCAST &&
    value.kind === "screencastGranted"
  ) {
    if (
      value.streams.length === 0 ||
      value.streams.length > g.YAS_MEDIA_MAX_SCREENCAST_CANDIDATES
    )
      throw new YasProtocolError(
        "invalid granted Media screencast stream count",
      );
    const surfaces = new Set<bigint>();
    const streams = new Set<bigint>();
    const writer = new YasWriter().u16(value.streams.length).u16(0);
    for (const stream of value.streams) {
      requireHandle(stream.surfaceHandle, "granted Media screencast Surface");
      requireHandle(stream.streamHandle, "granted Media screencast stream");
      if (
        surfaces.has(stream.surfaceHandle) ||
        streams.has(stream.streamHandle)
      )
        throw new YasProtocolError("duplicate granted Media screencast stream");
      surfaces.add(stream.surfaceHandle);
      streams.add(stream.streamHandle);
      writer.u64(stream.surfaceHandle).u64(stream.streamHandle);
    }
    return writer.finish();
  }
  throw new YasProtocolError("invalid granted Media portal metadata");
}

export function decodeMediaPortalGrantedMetadata(
  portalKind: number,
  bytes: Uint8Array,
): YasMediaPortalGrantedMetadata {
  if (portalKind === g.YAS_MEDIA_PORTAL_KIND_ACCESS) {
    const decoded = decodeMediaPortalReplyMetadata(
      portalKind,
      g.YAS_MEDIA_PORTAL_DECISION_GRANT,
      bytes,
    );
    if (decoded.kind !== "accessGrant")
      throw new YasProtocolError("invalid granted Media access metadata");
    return { kind: "accessGranted", choices: decoded.choices };
  }
  if (portalKind === g.YAS_MEDIA_PORTAL_KIND_SCREENCAST) {
    const cursor = new YasCursor(bytes);
    const count = cursor.u16("granted Media screencast stream count");
    if (
      cursor.u16("granted Media screencast reserved") !== 0 ||
      count === 0 ||
      count > g.YAS_MEDIA_MAX_SCREENCAST_CANDIDATES ||
      count > Math.floor(cursor.remaining / 16)
    )
      throw new YasProtocolError(
        "invalid granted Media screencast stream count",
      );
    const streams: { surfaceHandle: bigint; streamHandle: bigint }[] = [];
    for (let index = 0; index < count; index++)
      streams.push({
        surfaceHandle: cursor.u64("granted Media screencast Surface"),
        streamHandle: cursor.u64("granted Media screencast stream"),
      });
    cursor.end("granted Media screencast metadata");
    const value = { kind: "screencastGranted" as const, streams };
    encodeMediaPortalGrantedMetadata(portalKind, value);
    return value;
  }
  throw new YasProtocolError("invalid granted Media portal kind");
}

export function encodeMediaPortalRecordMetadata(
  portalKind: number,
  state: number,
  value: YasMediaPortalRecordMetadata,
): Uint8Array {
  if (state === g.YAS_MEDIA_PORTAL_PENDING && value.kind === "request") {
    if (
      (portalKind === g.YAS_MEDIA_PORTAL_KIND_ACCESS) !==
      (value.request.kind === "access")
    )
      throw new YasProtocolError("Media portal record kind mismatch");
    return encodeMediaPortalRequestMetadata(value.request);
  }
  if (state === g.YAS_MEDIA_PORTAL_GRANTED && value.kind === "grant")
    return encodeMediaPortalGrantedMetadata(portalKind, value.grant);
  if (
    (state === g.YAS_MEDIA_PORTAL_DENIED ||
      state === g.YAS_MEDIA_PORTAL_CANCELLED ||
      state === g.YAS_MEDIA_PORTAL_WITHDRAWN) &&
    value.kind === "empty"
  )
    return new Uint8Array();
  throw new YasProtocolError("invalid Media portal record metadata");
}

export function decodeMediaPortalRecordMetadata(
  portalKind: number,
  state: number,
  bytes: Uint8Array,
): YasMediaPortalRecordMetadata {
  if (state === g.YAS_MEDIA_PORTAL_PENDING)
    return {
      kind: "request",
      request: decodeMediaPortalRequestMetadata(portalKind, bytes),
    };
  if (state === g.YAS_MEDIA_PORTAL_GRANTED)
    return {
      kind: "grant",
      grant: decodeMediaPortalGrantedMetadata(portalKind, bytes),
    };
  if (
    state === g.YAS_MEDIA_PORTAL_DENIED ||
    state === g.YAS_MEDIA_PORTAL_CANCELLED ||
    state === g.YAS_MEDIA_PORTAL_WITHDRAWN
  ) {
    if (bytes.length !== 0)
      throw new YasProtocolError("nonempty terminal Media portal metadata");
    return { kind: "empty" };
  }
  throw new YasProtocolError("invalid Media portal state");
}

export function encodeMediaPortalRecord(
  value: YasMediaPortalRecord,
): Uint8Array {
  validatePortal(value);
  const metadata = encodeMediaPortalRecordMetadata(
    value.portalKind,
    value.state,
    value.metadata,
  );
  return new YasWriter()
    .u64(value.portalHandle)
    .u64(value.revision)
    .u16(value.portalKind)
    .u16(value.state)
    .bytes(value.ownerSession)
    .bytesU32(metadata)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeMediaPortalRecord(
  bytes: Uint8Array,
): YasMediaPortalRecord {
  const cursor = new YasCursor(bytes);
  const portalHandle = cursor.u64("Media portal");
  const revision = cursor.u64("Media portal revision");
  const portalKind = cursor.u16("Media portal kind");
  const state = cursor.u16("Media portal state");
  const ownerSession = new Uint8Array(cursor.take(16, "Media portal owner"));
  const value = {
    kind: "portal" as const,
    portalHandle,
    revision,
    portalKind,
    state,
    ownerSession,
    metadata: decodeMediaPortalRecordMetadata(
      portalKind,
      state,
      cursor.bytesU32("Media portal metadata"),
    ),
    extensions: decodeExtensions(
      cursor,
      new Set([g.YAS_MEDIA_PORTAL_ASSET_HASH_EXTENSION]),
      "Media portal extensions",
    ),
  };
  cursor.end("Media portal record");
  validatePortal(value);
  return value;
}

export function encodeMediaPlayerRecord(
  value: YasMediaPlayerRecord,
): Uint8Array {
  validatePlayer(value);
  return new YasWriter()
    .u64(value.playerHandle)
    .u64(value.revision)
    .u16(value.state)
    .u16(value.flags)
    .i64(value.positionUs)
    .i64(value.durationUs)
    .utf8U16(value.identity)
    .utf8U16(value.title)
    .utf8U16(value.artist)
    .utf8U16(value.album)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeMediaPlayerRecord(
  bytes: Uint8Array,
): YasMediaPlayerRecord {
  const cursor = new YasCursor(bytes);
  const value = {
    kind: "player" as const,
    playerHandle: cursor.u64("Media player"),
    revision: cursor.u64("Media player revision"),
    state: cursor.u16("Media player state"),
    flags: cursor.u16("Media player flags"),
    positionUs: cursor.i64("Media player position"),
    durationUs: cursor.i64("Media player duration"),
    identity: cursor.utf8U16("Media player identity"),
    title: cursor.utf8U16("Media player title"),
    artist: cursor.utf8U16("Media player artist"),
    album: cursor.utf8U16("Media player album"),
    extensions: decodeExtensions(
      cursor,
      new Set([
        g.YAS_MEDIA_PLAYER_ALBUM_ART_HASH_EXTENSION,
        g.YAS_MEDIA_PLAYER_ACTIVE_EXTENSION,
      ]),
      "Media player extensions",
    ),
  };
  cursor.end("Media player record");
  validatePlayer(value);
  return value;
}

type MediaMaps = {
  devices: Map<bigint, YasMediaDeviceRecord>;
  leases: Map<bigint, YasMediaLeaseRecord>;
  portals: Map<bigint, YasMediaPortalRecord>;
  players: Map<bigint, YasMediaPlayerRecord>;
};

export class YasMediaCatalog {
  private maps = emptyMaps();
  private staging: MediaMaps | null = null;
  private retention: YasStateCatalogueRetention<string>;
  private stagingRetention: YasStateCatalogueRetention<string> | null = null;
  private subscription: YasStateSubscription | null = null;
  private revision = 0n;
  private listeners = new Set<(snapshot: YasMediaSnapshot) => void>();
  private pendingFirstSnapshots = new Set<(error: unknown) => void>();
  private readonly removeInvalidation: () => void;
  private pendingWatch: Promise<void> | null = null;
  private pendingWatchCancel: ((error: unknown) => void) | null = null;
  private watchEpoch = 0;
  private disposed = false;

  constructor(private readonly connection: YasConnection) {
    this.retention = YasStateCatalogueRetention.forConnection(connection);
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family === undefined || family === g.YAS_FAMILY_MEDIA) {
        this.cancelPendingWatch(
          new YasProtocolError("Media catalogue was invalidated"),
        );
        this.resetLocal();
      }
    });
  }

  get snapshot(): YasMediaSnapshot {
    return {
      revision: this.revision,
      devices: [...this.maps.devices.values()],
      leases: [...this.maps.leases.values()],
      portals: [...this.maps.portals.values()],
      players: [...this.maps.players.values()],
    };
  }

  subscribe(listener: (snapshot: YasMediaSnapshot) => void): () => void {
    if (this.disposed) throw new Error("Media catalogue is disposed");
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
  ): Promise<YasMediaSnapshot> {
    if (this.disposed) throw new Error("Media catalogue is disposed");
    if (this.revision !== 0n && this.subscription?.active) return this.snapshot;
    let remove: (() => void) | undefined;
    let rejectPending!: (error: unknown) => void;
    const result = new Promise<YasMediaSnapshot>((resolve, reject) => {
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
      return Promise.reject(new Error("Media catalogue is disposed"));
    if (this.subscription?.active) return Promise.resolve();
    if (this.pendingWatch) return this.pendingWatch;
    this.resetLocal();
    const epoch = this.watchEpoch;
    const watched = YasStateSubscription.watch(
      this.connection,
      g.YAS_FAMILY_MEDIA,
      g.YAS_MEDIA_WATCH,
      g.YAS_MEDIA_UNWATCH,
      g.YAS_MEDIA_STATE,
      g.YAS_MEDIA_STATE_ACK,
      options,
      (batch) => {
        if (!this.disposed && epoch === this.watchEpoch) this.apply(batch);
      },
    ).then(async (subscription) => {
      if (this.disposed || epoch !== this.watchEpoch) {
        await subscription.unwatch().catch(() => undefined);
        throw new YasProtocolError("Media catalogue watch was cancelled");
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
      new YasProtocolError("Media catalogue watch was cancelled"),
    );
    const subscription = this.subscription;
    this.subscription = null;
    if (!this.disposed) this.clearState();
    await subscription?.unwatch();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    const disposalError = new Error("Media catalogue is disposed");
    this.cancelPendingWatch(disposalError);
    this.removeInvalidation();
    for (const reject of [...this.pendingFirstSnapshots]) reject(disposalError);
    this.pendingFirstSnapshots.clear();
    this.listeners.clear();
    const subscription = this.subscription;
    this.subscription = null;
    this.retention.dispose();
    this.stagingRetention?.dispose();
    this.maps = emptyMaps();
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
      this.staging = emptyMaps();
      this.stagingRetention = YasStateCatalogueRetention.forConnection(
        this.connection,
      );
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_RECORDS) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("Media snapshot records without begin");
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
        throw new YasProtocolError("Media snapshot end without begin");
      try {
        this.applyRecords(this.staging, this.stagingRetention, batch.records);
        this.validateCatalog(this.staging);
      } catch (error) {
        this.discardStaging();
        throw error;
      }
      const previousRetention = this.retention;
      this.maps = this.staging;
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
      let next: MediaMaps;
      try {
        next = cloneMaps(this.maps);
        this.applyRecords(next, retention, batch.records);
        this.validateCatalog(next);
      } catch (error) {
        retention.dispose();
        throw error;
      }
      const previousRetention = this.retention;
      this.maps = next;
      this.retention = retention;
      previousRetention.dispose();
      this.revision = batch.toRevision;
      this.emit();
    }
  }

  private applyRecords(
    target: MediaMaps,
    retention: YasStateCatalogueRetention<string>,
    records: readonly YasTypedRecord[],
  ): void {
    for (const action of records) {
      const cursor = new YasCursor(action.body);
      const entity = cursor.u16("Media state entity");
      if (cursor.u16("Media state reserved") !== 0)
        throw new YasProtocolError("Media state reserved field is nonzero");
      const body = new Uint8Array(cursor.take(cursor.remaining));
      if (entity === g.YAS_MEDIA_ENTITY_DEVICE) {
        this.admitEntity(target.devices, action.kind, this.deviceLimit());
        applyEntity(
          target.devices,
          action.kind,
          body,
          decodeMediaDeviceRecord,
          encodeMediaDeviceRecord,
          (v) => v.deviceHandle,
          new Set(),
          retention,
          "device",
        );
      } else if (entity === g.YAS_MEDIA_ENTITY_LEASE) {
        this.admitEntity(target.leases, action.kind, this.leaseLimit());
        applyEntity(
          target.leases,
          action.kind,
          body,
          decodeMediaLeaseRecord,
          encodeMediaLeaseRecord,
          (v) => v.leaseHandle,
          new Set(),
          retention,
          "lease",
        );
      } else if (entity === g.YAS_MEDIA_ENTITY_PORTAL) {
        this.admitEntity(target.portals, action.kind, this.portalLimit());
        applyEntity(
          target.portals,
          action.kind,
          body,
          decodeMediaPortalRecord,
          encodeMediaPortalRecord,
          (v) => v.portalHandle,
          new Set([g.YAS_MEDIA_PORTAL_ASSET_HASH_EXTENSION]),
          retention,
          "portal",
        );
      } else if (entity === g.YAS_MEDIA_ENTITY_PLAYER) {
        this.admitEntity(target.players, action.kind, this.playerLimit());
        applyEntity(
          target.players,
          action.kind,
          body,
          decodeMediaPlayerRecord,
          encodeMediaPlayerRecord,
          (v) => v.playerHandle,
          new Set([
            g.YAS_MEDIA_PLAYER_ALBUM_ART_HASH_EXTENSION,
            g.YAS_MEDIA_PLAYER_ACTIVE_EXTENSION,
          ]),
          retention,
          "player",
        );
      } else throw new YasProtocolError("unknown Media state entity");
    }
  }

  private validateCatalog(maps: MediaMaps): void {
    if (
      maps.devices.size > this.deviceLimit() ||
      maps.leases.size > this.leaseLimit() ||
      maps.portals.size > this.portalLimit() ||
      maps.players.size > this.playerLimit()
    )
      throw new YasProtocolError(
        "Media catalogue exceeds its negotiated entity limits",
      );
  }

  private admitEntity(
    records: ReadonlyMap<bigint, unknown>,
    action: number,
    limit: number,
  ): void {
    if (action === YAS_STATE_ADD && records.size >= limit)
      throw new YasProtocolError(
        "Media catalogue exceeds its negotiated entity limits",
      );
  }

  private entityLimit(tag: number, hardMaximum: number): number {
    return negotiatedStateLimitU32(
      this.connection,
      g.YAS_FAMILY_MEDIA,
      g.YAS_MEDIA_VERSION,
      tag,
      hardMaximum,
    );
  }

  private deviceLimit(): number {
    return this.entityLimit(
      g.YAS_MEDIA_LIMIT_MAX_DEVICES,
      g.YAS_MEDIA_MAX_DEVICES,
    );
  }

  private leaseLimit(): number {
    return this.entityLimit(
      g.YAS_MEDIA_LIMIT_MAX_LEASES_PER_SESSION,
      g.YAS_MEDIA_MAX_LEASES_PER_SESSION,
    );
  }

  private portalLimit(): number {
    return this.entityLimit(
      g.YAS_MEDIA_LIMIT_MAX_PORTALS_PER_SESSION,
      g.YAS_MEDIA_MAX_PORTALS_PER_SESSION,
    );
  }

  private playerLimit(): number {
    return this.entityLimit(
      g.YAS_MEDIA_LIMIT_MAX_PLAYERS,
      g.YAS_MEDIA_MAX_PLAYERS,
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
    this.maps = emptyMaps();
    this.staging = null;
    this.retention = YasStateCatalogueRetention.forConnection(this.connection);
    this.stagingRetention = null;
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

export class YasMediaClient {
  readonly catalog: YasMediaCatalog;
  private readonly transfers;
  private portalListeners = new Set<(request: YasMediaPortalRequest) => void>();
  private frameListeners = new Set<(frame: YasMediaFrame) => void>();
  private ackListeners = new Set<(ack: YasMediaFrameAck) => void>();
  private statusListeners = new Set<(status: YasMediaStreamStatus) => void>();
  private terminalStreams = new Set<bigint>();
  private removeListeners: (() => void)[];

  constructor(readonly connection: YasConnection) {
    this.catalog = new YasMediaCatalog(connection);
    this.transfers = transfersFor(connection);
    this.removeListeners = [
      connection.onInvalidation(({ family }) => {
        if (family === undefined || family === g.YAS_FAMILY_MEDIA)
          this.terminalStreams.clear();
      }),
      connection.onEvent(
        g.YAS_FAMILY_MEDIA,
        g.YAS_MEDIA_PORTAL_REQUEST,
        ({ payload }) => {
          const value = decodeMediaPortalRequest(payload);
          for (const listener of this.portalListeners) listener(value);
        },
      ),
      connection.onEvent(
        g.YAS_FAMILY_MEDIA,
        g.YAS_MEDIA_FRAME,
        ({ payload }) => {
          const value = decodeMediaFrame(payload);
          // Optional datagrams are allowed to arrive after the authoritative
          // reliable final status. Resource handles are boot-monotonic, so a
          // terminal stream can never become active again in this connection.
          if (this.terminalStreams.has(value.streamHandle)) return;
          for (const listener of this.frameListeners) listener(value);
        },
      ),
      connection.onEvent(
        g.YAS_FAMILY_MEDIA,
        g.YAS_MEDIA_FRAME_ACK,
        ({ payload }) => {
          const value = decodeMediaFrameAck(payload);
          for (const listener of this.ackListeners) listener(value);
        },
      ),
      connection.onEvent(
        g.YAS_FAMILY_MEDIA,
        g.YAS_MEDIA_STREAM_STATUS,
        ({ payload }) => {
          const value = decodeMediaStreamStatus(payload);
          if (
            value.status === g.YAS_MEDIA_STREAM_CLOSED ||
            value.status === g.YAS_MEDIA_STREAM_ERROR
          )
            this.terminalStreams.add(value.streamHandle);
          for (const listener of this.statusListeners) listener(value);
        },
      ),
    ];
  }

  list(options: YasWatchOptions = {}): Promise<YasMediaSnapshot> {
    return this.catalog.firstSnapshot(options);
  }

  onPortalRequest(
    listener: (request: YasMediaPortalRequest) => void,
  ): () => void {
    this.portalListeners.add(listener);
    return () => this.portalListeners.delete(listener);
  }

  onFrame(listener: (frame: YasMediaFrame) => void): () => void {
    this.frameListeners.add(listener);
    return () => this.frameListeners.delete(listener);
  }

  onFrameAck(listener: (ack: YasMediaFrameAck) => void): () => void {
    this.ackListeners.add(listener);
    return () => this.ackListeners.delete(listener);
  }

  onStreamStatus(listener: (status: YasMediaStreamStatus) => void): () => void {
    this.statusListeners.add(listener);
    return () => this.statusListeners.delete(listener);
  }

  openOutput(value: YasMediaOpenOutput): Promise<YasMediaOpenOutputResult> {
    return this.connection.requestDecoded(
      g.YAS_FAMILY_MEDIA,
      g.YAS_MEDIA_OPEN_OUTPUT,
      encodeMediaOpenOutput(value),
      decodeMediaOpenOutputResult,
    );
  }

  acquireDevice(
    value: YasMediaAcquireDevice,
  ): Promise<YasMediaAcquireDeviceResult> {
    return this.connection.requestDecoded(
      g.YAS_FAMILY_MEDIA,
      g.YAS_MEDIA_ACQUIRE_DEVICE,
      encodeMediaAcquireDevice(value),
      decodeMediaAcquireDeviceResult,
    );
  }

  async releaseDevice(
    leaseHandle: bigint,
    operationId: Uint8Array,
    extensions: readonly YasExtension[] = [],
  ): Promise<void> {
    await this.connection.request(
      g.YAS_FAMILY_MEDIA,
      g.YAS_MEDIA_RELEASE_DEVICE,
      encodeMediaHandleOperation(leaseHandle, operationId, extensions),
    );
  }

  async closeStream(
    streamHandle: bigint,
    operationId: Uint8Array,
    extensions: readonly YasExtension[] = [],
  ): Promise<void> {
    await this.connection.request(
      g.YAS_FAMILY_MEDIA,
      g.YAS_MEDIA_CLOSE_STREAM,
      encodeMediaHandleOperation(streamHandle, operationId, extensions),
    );
  }

  async portalReply(value: YasMediaPortalReply): Promise<void> {
    await this.connection.request(
      g.YAS_FAMILY_MEDIA,
      g.YAS_MEDIA_PORTAL_REPLY,
      encodeMediaPortalReply(value),
    );
  }

  async portalClose(value: YasMediaPortalClose): Promise<void> {
    await this.connection.request(
      g.YAS_FAMILY_MEDIA,
      g.YAS_MEDIA_PORTAL_CLOSE,
      encodeMediaPortalClose(value),
    );
  }

  async playerAction(value: YasMediaPlayerAction): Promise<void> {
    await this.connection.request(
      g.YAS_FAMILY_MEDIA,
      g.YAS_MEDIA_PLAYER_ACTION,
      encodeMediaPlayerAction(value),
    );
  }

  async fetchAsset(
    contentHash: Uint8Array,
    initialReceiveCredit = 1024n * 1024n,
    extensions: readonly YasExtension[] = [],
  ): Promise<YasMediaContent> {
    const lease = this.transfers.reserveReceiveCredit(
      initialReceiveCredit,
      1024n,
    );
    let accepted = false;
    try {
      return await this.connection.requestDecoded(
        g.YAS_FAMILY_MEDIA,
        g.YAS_MEDIA_FETCH_ASSET,
        encodeMediaFetchAsset(contentHash, lease.bytes, extensions),
        (body) => {
          const delivery = decodeInlineOrTransfer(body);
          if (delivery.delivery === "inline") {
            if (delivery.bytes.length > g.YAS_MEDIA_MAX_INLINE_ASSET_BYTES)
              throw new YasProtocolError(
                "Media inline asset exceeds its limit",
              );
            lease.release();
            accepted = true;
            const bytes = new Uint8Array(delivery.bytes);
            return {
              byteLength: delivery.byteLength,
              contentHash: delivery.contentHash,
              bytes: async () => new Uint8Array(bytes),
            };
          }
          validateAssetTransfer(delivery.descriptor);
          const transfer = this.transfers.acceptServerDescriptor(
            delivery.descriptor,
            lease,
          );
          accepted = true;
          return transferContent(
            delivery.byteLength,
            delivery.contentHash,
            transfer,
          );
        },
      );
    } catch (error) {
      if (!accepted) lease.release();
      throw error;
    }
  }

  sendFrame(value: YasMediaFrame): void {
    const payload = encodeMediaFrame(value);
    const reliableOnly =
      g.YAS_MEDIA_FRAME_KEYFRAME |
      g.YAS_MEDIA_FRAME_CODEC_CONFIG |
      g.YAS_MEDIA_FRAME_END_OF_STREAM;
    if (
      value.flags & g.YAS_MEDIA_FRAME_DISCARDABLE &&
      !(value.flags & reliableOnly) &&
      this.connection.sendDatagramEvent(
        g.YAS_FAMILY_MEDIA,
        g.YAS_MEDIA_FRAME,
        payload,
      )
    )
      return;
    this.connection.sendEvent(g.YAS_FAMILY_MEDIA, g.YAS_MEDIA_FRAME, payload);
  }

  sendFrameAck(value: YasMediaFrameAck): void {
    this.connection.sendEvent(
      g.YAS_FAMILY_MEDIA,
      g.YAS_MEDIA_FRAME_ACK,
      encodeMediaFrameAck(value),
    );
  }

  dispose(): void {
    for (const remove of this.removeListeners) remove();
    this.removeListeners = [];
    this.catalog.dispose();
    this.portalListeners.clear();
    this.frameListeners.clear();
    this.ackListeners.clear();
    this.statusListeners.clear();
    this.terminalStreams.clear();
  }
}

function applyEntity<
  T extends { revision: bigint; extensions: readonly YasExtension[] },
>(
  target: Map<bigint, T>,
  action: number,
  body: Uint8Array,
  decode: (bytes: Uint8Array) => T,
  encode: (value: T) => Uint8Array,
  keyOf: (record: T) => bigint,
  extensionTags: ReadonlySet<number>,
  retention: YasStateCatalogueRetention<string>,
  retentionPrefix: string,
): void {
  if (action === YAS_STATE_ADD || action === YAS_STATE_REPLACE) {
    const record = detachStateRetainedValue(decode(body));
    const key = keyOf(record);
    const exists = target.has(key);
    if ((action === YAS_STATE_ADD) === exists)
      throw new YasProtocolError("Media ADD/REPLACE precondition failed");
    retention.upsert(
      `${retentionPrefix}:${key}`,
      Math.max(encode(record).length, estimateStateRetainedBytes(record)),
    );
    target.set(key, record);
    return;
  }
  const cursor = new YasCursor(body);
  const handle = cursor.u64("Media entity handle");
  const revision = cursor.u64("Media entity revision");
  requireHandle(handle, "Media entity");
  requireRevision(revision, "Media entity revision");
  if (action === YAS_STATE_PATCH) {
    const extensions = decodeExtensions(
      cursor,
      extensionTags,
      "Media entity patch",
    );
    cursor.end("Media PATCH");
    const previous = target.get(handle);
    if (!previous)
      throw new YasProtocolError("Media PATCH names an unknown entity");
    const next = detachStateRetainedValue({
      ...previous,
      revision,
      extensions: mergeExtensions(previous.extensions, extensions),
    });
    retention.upsert(
      `${retentionPrefix}:${handle}`,
      Math.max(encode(next).length, estimateStateRetainedBytes(next)),
    );
    target.set(handle, next);
  } else if (action === YAS_STATE_REMOVE) {
    cursor.end("Media REMOVE");
    if (!target.has(handle))
      throw new YasProtocolError("Media REMOVE names an unknown entity");
    retention.remove(`${retentionPrefix}:${handle}`);
    target.delete(handle);
  } else throw new YasProtocolError("unsupported Media state record kind");
}

function emptyMaps(): MediaMaps {
  return {
    devices: new Map(),
    leases: new Map(),
    portals: new Map(),
    players: new Map(),
  };
}

function cloneMaps(value: MediaMaps): MediaMaps {
  return {
    devices: new Map(value.devices),
    leases: new Map(value.leases),
    portals: new Map(value.portals),
    players: new Map(value.players),
  };
}

function validateMediaFormat(value: YasMediaFormat): void {
  const audio = isAudioCodec(value.codec);
  const video = isVideoCodec(value.codec);
  if (
    (!audio && !video) ||
    (audio &&
      (value.channels === 0 ||
        value.sampleRate === 0 ||
        value.width !== 0 ||
        value.height !== 0 ||
        value.frameRateMilli !== 0)) ||
    (video &&
      (value.channels !== 0 ||
        value.sampleRate !== 0 ||
        value.width === 0 ||
        value.height === 0 ||
        value.frameRateMilli === 0))
  )
    throw new YasProtocolError("invalid Media format");
}

function validateFormats(
  formats: readonly YasMediaFormat[],
  audio?: boolean,
): void {
  if (formats.length === 0 || formats.length > g.YAS_MEDIA_MAX_FORMATS)
    throw new YasProtocolError("invalid Media format count");
  const codecs = new Set<number>();
  for (const format of formats) {
    validateMediaFormat(format);
    if (
      codecs.has(format.codec) ||
      (audio !== undefined && isAudioCodec(format.codec) !== audio)
    )
      throw new YasProtocolError("invalid Media format set");
    codecs.add(format.codec);
  }
}

function isAudioCodec(codec: number): boolean {
  return (
    codec === g.YAS_MEDIA_CODEC_PCM_S16LE ||
    codec === g.YAS_MEDIA_CODEC_PCM_F32LE ||
    codec === g.YAS_MEDIA_CODEC_OPUS
  );
}

function isVideoCodec(codec: number): boolean {
  return (
    codec === g.YAS_MEDIA_CODEC_H264 ||
    codec === g.YAS_MEDIA_CODEC_H264_444 ||
    codec === g.YAS_MEDIA_CODEC_AV1 ||
    codec === g.YAS_MEDIA_CODEC_AV1_444 ||
    codec === g.YAS_MEDIA_CODEC_VP9 ||
    codec === g.YAS_MEDIA_CODEC_MJPEG
  );
}

function validateMediaFrame(value: YasMediaFrame): void {
  requireHandle(value.streamHandle, "Media stream");
  if (
    (!isAudioCodec(value.codecVersion) && !isVideoCodec(value.codecVersion)) ||
    value.flags & ~g.YAS_MEDIA_FRAME_FLAGS_MASK ||
    value.fragmentCount === 0 ||
    value.fragmentIndex >= value.fragmentCount ||
    value.completeLength === 0 ||
    value.payload.length === 0 ||
    value.payload.length > value.completeLength
  )
    throw new YasProtocolError("invalid Media frame");
}

function validateDevice(value: YasMediaDeviceRecord): void {
  requireHandle(value.deviceHandle, "Media device");
  requireRevision(value.revision, "Media device revision");
  if (
    value.deviceKind > g.YAS_MEDIA_KIND_CAMERA ||
    value.state > g.YAS_MEDIA_DEVICE_PERMISSION_REQUIRED ||
    value.flags & ~g.YAS_MEDIA_DEVICE_FLAGS_MASK
  )
    throw new YasProtocolError("invalid Media device record");
  validateFormats(value.formats, value.deviceKind !== g.YAS_MEDIA_KIND_CAMERA);
}

function validateLease(value: YasMediaLeaseRecord): void {
  requireHandle(value.leaseHandle, "Media lease");
  requireHandle(value.deviceHandle, "Media device");
  requireRevision(value.revision, "Media lease revision");
  if (
    value.ownerSession.length !== 16 ||
    value.lifecycle > g.YAS_MEDIA_LEASE_RELEASED
  )
    throw new YasProtocolError("invalid Media lease record");
}

function validatePortal(value: YasMediaPortalRecord): void {
  requireHandle(value.portalHandle, "Media portal");
  requireRevision(value.revision, "Media portal revision");
  if (
    value.ownerSession.length !== 16 ||
    value.portalKind > g.YAS_MEDIA_PORTAL_KIND_SCREENCAST ||
    value.state > g.YAS_MEDIA_PORTAL_WITHDRAWN
  )
    throw new YasProtocolError("invalid Media portal record");
  const metadata = encodeMediaPortalRecordMetadata(
    value.portalKind,
    value.state,
    value.metadata,
  );
  if (metadata.length > g.YAS_MEDIA_MAX_PORTAL_METADATA_BYTES)
    throw new YasProtocolError("Media portal record metadata is too large");
  validateHashExtension(
    value.extensions,
    g.YAS_MEDIA_PORTAL_ASSET_HASH_EXTENSION,
    "Media portal asset hash",
  );
}

function validatePlayer(value: YasMediaPlayerRecord): void {
  requireHandle(value.playerHandle, "Media player");
  requireRevision(value.revision, "Media player revision");
  if (
    value.state > g.YAS_MEDIA_PLAYER_PLAYING ||
    value.flags & ~g.YAS_MEDIA_PLAYER_FLAGS_MASK ||
    value.positionUs < 0n ||
    value.durationUs < -1n
  )
    throw new YasProtocolError("invalid Media player record");
  validateHashExtension(
    value.extensions,
    g.YAS_MEDIA_PLAYER_ALBUM_ART_HASH_EXTENSION,
    "Media album art hash",
  );
  mediaPlayerActive(value);
}

/** Exact server-selected player state. Absence is unknown, never inferred. */
export function mediaPlayerActive(
  value: Pick<YasMediaPlayerRecord, "extensions">,
): boolean | null {
  const extension = value.extensions.find(
    (entry) => entry.tag === g.YAS_MEDIA_PLAYER_ACTIVE_EXTENSION,
  );
  if (!extension) return null;
  if (
    extension.value.length !== 1 ||
    (extension.value[0] !== 0 && extension.value[0] !== 1)
  )
    throw new YasProtocolError("Media player active state is invalid");
  return extension.value[0] === 1;
}

function validateHashExtension(
  extensions: readonly YasExtension[],
  tag: number,
  context: string,
): void {
  const extension = extensions.find((entry) => entry.tag === tag);
  if (extension && extension.value.length !== 32)
    throw new YasProtocolError(`${context} is not 32 bytes`);
}

function validatePortalText(
  value: string,
  maximumBytes: number,
  required: boolean,
): void {
  if (
    (required && value.length === 0) ||
    value.includes("\0") ||
    textEncoder.encode(value).length > maximumBytes
  )
    throw new YasProtocolError("invalid Media portal text");
}

function validateAssetTransfer(descriptor: YasTransferDescriptor): void {
  if (
    descriptor.mode !== YAS_TRANSFER_MODE_BYTE ||
    descriptor.direction !== YAS_TRANSFER_SENDER_TO_RECEIVER ||
    descriptor.contentFamily !== g.YAS_FAMILY_MEDIA ||
    descriptor.contentKind !== g.YAS_MEDIA_ASSET_CONTENT_KIND ||
    descriptor.contentVersion !== g.YAS_MEDIA_VERSION ||
    !descriptor.sensitiveContent
  )
    throw new YasProtocolError("invalid Media asset Transfer descriptor");
}

function transferContent(
  byteLength: bigint,
  contentHash: Uint8Array,
  transfer: YasTransfer,
): YasMediaContent {
  let collected: Promise<Uint8Array> | undefined;
  return {
    byteLength,
    contentHash,
    bytes: () => (collected ??= transfer.collect(byteLength)),
  };
}

function requireHandle(value: bigint, context: string): void {
  if (value === 0n) throw new YasProtocolError(`${context} is zero`);
}

function requireRevision(value: bigint, context: string): void {
  if (value === 0n) throw new YasProtocolError(`${context} is zero`);
}

function requireOperationId(value: Uint8Array, context: string): void {
  if (value.length !== 16)
    throw new YasProtocolError(`${context} operation ID is not 16 bytes`);
}

function requireHash(value: Uint8Array, context: string): void {
  if (value.length !== 32)
    throw new YasProtocolError(`${context} is not 32 bytes`);
}

function requireZero(bytes: Uint8Array, context: string): void {
  if (bytes.some((byte) => byte !== 0))
    throw new YasProtocolError(`${context} reserved bytes are nonzero`);
}

function mergeExtensions(
  previous: readonly YasExtension[],
  patch: readonly YasExtension[],
): YasExtension[] {
  const byTag = new Map(
    previous.map((extension) => [extension.tag, extension]),
  );
  for (const extension of patch) byTag.set(extension.tag, extension);
  return [...byTag.values()].sort((left, right) => left.tag - right.tag);
}
