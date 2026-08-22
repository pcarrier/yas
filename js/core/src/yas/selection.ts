/** YAS Selection family v1 codecs and browser client. */

import {
  YAS_FAMILY_SELECTION,
  YAS_SELECTION_ACTION_MASK,
  YAS_SELECTION_CLEAR,
  YAS_SELECTION_DRAG_BEGIN,
  YAS_SELECTION_DRAG_CANCEL,
  YAS_SELECTION_DRAG_DROP,
  YAS_SELECTION_DRAG_DROP_ITEMS_EXTENSION,
  YAS_SELECTION_DRAG_ENTER,
  YAS_SELECTION_DRAG_LEAVE,
  YAS_SELECTION_DRAG_MOTION,
  YAS_SELECTION_ENTITY_DRAG,
  YAS_SELECTION_ENTITY_SLOT,
  YAS_SELECTION_GET,
  YAS_SELECTION_GET_TARGET_DRAG,
  YAS_SELECTION_GET_TARGET_SLOT,
  YAS_SELECTION_ITEM_CONTENT_KIND,
  YAS_SELECTION_LIMIT_MAX_ACTIVE_DRAGS_PER_SESSION,
  YAS_SELECTION_LIMIT_MAX_ITEMS,
  YAS_SELECTION_LIMIT_MAX_MUTATION_REPLAYS,
  YAS_SELECTION_MAX_ACTIVE_DRAGS_PER_SESSION,
  YAS_SELECTION_MAX_INLINE_BYTES,
  YAS_SELECTION_MAX_ITEM_NAME_BYTES,
  YAS_SELECTION_MAX_ITEMS,
  YAS_SELECTION_MAX_MIME_BYTES,
  YAS_SELECTION_MAX_MUTATION_REPLAYS,
  YAS_SELECTION_OWNER_EXTERNAL,
  YAS_SELECTION_OWNER_NONE,
  YAS_SELECTION_SET,
  YAS_SELECTION_SET_BEGIN,
  YAS_SELECTION_SET_COMMIT,
  YAS_SELECTION_SLOT_CLIPBOARD,
  YAS_SELECTION_SLOT_PRIMARY,
  YAS_SELECTION_STATE,
  YAS_SELECTION_STATE_ACK,
  YAS_SELECTION_UNWATCH,
  YAS_SELECTION_VERSION,
  YAS_SELECTION_WATCH,
  YAS_STATUS_NOT_FOUND,
  YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION,
} from "./generated";
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
  estimateStateRetainedBytes,
  negotiatedStateLimitU32,
  type YasStateBatch,
  type YasWatchOptions,
} from "./state";
import {
  YAS_TRANSFER_MODE_BYTE,
  YAS_TRANSFER_RECEIVER_TO_SENDER,
  YAS_TRANSFER_SENDER_TO_RECEIVER,
  decodeInlineOrTransfer,
  decodeTransferDescriptor,
  encodeInlineOrTransfer,
  requireTransferUploadStage,
  transfersFor,
  type YasTransfer,
  type YasTransferDescriptor,
} from "./transfer";
import {
  YasCursor,
  YasProtocolError,
  YasResultError,
  YasWriter,
  YAS_STATUS_OK,
  decodeExtensions,
  encodeExtensions,
  type YasExtension,
  type YasTypedRecord,
} from "./wire";

export {
  YAS_FAMILY_SELECTION,
  YAS_SELECTION_ACTION_COPY,
  YAS_SELECTION_ACTION_LINK,
  YAS_SELECTION_ACTION_MASK,
  YAS_SELECTION_ACTION_MOVE,
  YAS_SELECTION_CLEAR,
  YAS_SELECTION_DRAG_BEGIN,
  YAS_SELECTION_DRAG_CANCEL,
  YAS_SELECTION_DRAG_DROP,
  YAS_SELECTION_DRAG_ENTER,
  YAS_SELECTION_DRAG_LEAVE,
  YAS_SELECTION_DRAG_MOTION,
  YAS_SELECTION_ENTITY_DRAG,
  YAS_SELECTION_ENTITY_SLOT,
  YAS_SELECTION_GET,
  YAS_SELECTION_GET_TARGET_DRAG,
  YAS_SELECTION_GET_TARGET_SLOT,
  YAS_SELECTION_ITEM_CONTENT_KIND,
  YAS_SELECTION_MAX_INLINE_BYTES,
  YAS_SELECTION_MAX_ITEMS,
  YAS_SELECTION_MAX_MIME_BYTES,
  YAS_SELECTION_OWNER_EXTERNAL,
  YAS_SELECTION_OWNER_NONE,
  YAS_SELECTION_OWNER_SESSION,
  YAS_SELECTION_OWNER_SURFACE,
  YAS_SELECTION_SET,
  YAS_SELECTION_SET_BEGIN,
  YAS_SELECTION_SET_COMMIT,
  YAS_SELECTION_SLOT_CLIPBOARD,
  YAS_SELECTION_SLOT_PRIMARY,
  YAS_SELECTION_STATE,
  YAS_SELECTION_STATE_ACK,
  YAS_SELECTION_UNWATCH,
  YAS_SELECTION_VERSION,
  YAS_SELECTION_WATCH,
} from "./generated";

export interface YasSelectionInlineItem {
  mime: string;
  data: Uint8Array;
}

export interface YasSelectionUploadItem {
  mime: string;
  byteLength: bigint;
  contentHash: Uint8Array;
  initialReceiveCredit: bigint;
}

export interface YasSelectionUploadBatch {
  stagingHandle: bigint;
  transfers: readonly YasTransfer[];
  extensions: readonly YasExtension[];
}

interface YasSelectionBeginOperation {
  payloadKey: string;
  pending: Promise<YasSelectionUploadBatch> | null;
  batch: YasSelectionUploadBatch | null;
  identity: YasSelectionBeginIdentity | null;
  settled: boolean;
}

interface YasSelectionBeginIdentity {
  stagingHandle: bigint;
  transferIds: readonly number[];
}

interface YasSelectionStageOwnership {
  operationKey: string;
  batch: YasSelectionUploadBatch;
  expiresServerNs: bigint;
  removeListeners: (() => void)[];
}

export type YasSelectionGetTarget =
  | { kind: "slot"; slot: number; revision: bigint }
  | {
      kind: "drag";
      dragHandle: bigint;
      revision: bigint;
      itemIndex: number;
    };

export interface YasSelectionGet {
  target: YasSelectionGetTarget;
  initialReceiveCredit: bigint;
  mime: string;
  extensions?: readonly YasExtension[];
}

export interface YasSelectionContent {
  byteLength: bigint;
  contentHash: Uint8Array;
  bytes(): Promise<Uint8Array>;
}

export interface YasSelectionDragItem {
  name: string;
  mimeTypes: readonly string[];
}

export interface YasSelectionDragPosition {
  dragHandle: bigint;
  revision: bigint;
  targetSurface: bigint;
  x32_32: bigint;
  y32_32: bigint;
  actions: number;
}

export interface YasSelectionDragLeave {
  dragHandle: bigint;
  revision: bigint;
  targetSurface: bigint;
}

export interface YasSelectionDragDrop {
  dragHandle: bigint;
  revision: bigint;
  operationId: Uint8Array;
  selectedAction: number;
  extensions: readonly YasExtension[];
}

export interface YasSelectionDragDropItem {
  name: string;
  selectedMime: string;
}

export function selectionDragDropItemsExtension(
  items: readonly YasSelectionDragDropItem[],
): YasExtension {
  if (items.length === 0 || items.length > YAS_SELECTION_MAX_ITEMS)
    throw new YasProtocolError("invalid Selection DRAG_DROP item count");
  const writer = new YasWriter().u16(items.length);
  for (const item of items) {
    validateDragName(item.name);
    validateMime(item.selectedMime);
    writer.utf8U16(item.name).utf8U16(item.selectedMime);
  }
  return {
    tag: YAS_SELECTION_DRAG_DROP_ITEMS_EXTENSION,
    required: true,
    value: writer.finish(),
  };
}

export function selectionDragDropItems(
  extensions: readonly YasExtension[],
  expectedCount?: number,
): readonly YasSelectionDragDropItem[] | undefined {
  let result: YasSelectionDragDropItem[] | undefined;
  for (const extension of extensions) {
    if (extension.tag !== YAS_SELECTION_DRAG_DROP_ITEMS_EXTENSION) {
      if (extension.required)
        throw new YasProtocolError(
          "unknown required Selection DRAG_DROP extension",
        );
      continue;
    }
    if (!extension.required)
      throw new YasProtocolError("optional Selection DRAG_DROP items");
    const cursor = new YasCursor(extension.value);
    const count = cursor.u16("Selection DRAG_DROP item count");
    if (
      count === 0 ||
      count > YAS_SELECTION_MAX_ITEMS ||
      count > Math.floor(cursor.remaining / 4) ||
      (expectedCount !== undefined && count !== expectedCount)
    )
      throw new YasProtocolError("invalid Selection DRAG_DROP item count");
    result = [];
    for (let index = 0; index < count; index++) {
      const name = cursor.utf8U16("Selection DRAG_DROP item name");
      const selectedMime = cursor.utf8U16("Selection DRAG_DROP selected MIME");
      validateDragName(name);
      validateMime(selectedMime);
      result.push({ name, selectedMime });
    }
    cursor.end("Selection DRAG_DROP items");
  }
  return result;
}

export function encodeSelectionDragDrop(
  value: YasSelectionDragDrop,
): Uint8Array {
  requireHandle(value.dragHandle, "Selection drag handle");
  requireRevision(value.revision, "Selection drag revision");
  requireOperationId(value.operationId, "Selection DRAG_DROP");
  validateActions(value.selectedAction, false, true);
  if (selectionDragDropItems(value.extensions) === undefined)
    throw new YasProtocolError(
      "Selection DRAG_DROP is missing its required items extension",
    );
  return new YasWriter()
    .u64(value.dragHandle)
    .u64(value.revision)
    .bytes(value.operationId)
    .u16(value.selectedAction)
    .u16(0)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeSelectionDragDrop(
  bytes: Uint8Array,
): YasSelectionDragDrop {
  const cursor = new YasCursor(bytes);
  const dragHandle = cursor.u64("Selection drag handle");
  const revision = cursor.u64("Selection drag revision");
  const operationId = new Uint8Array(cursor.take(16, "Selection operation ID"));
  const selectedAction = cursor.u16("Selection selected action");
  if (cursor.u16("Selection DRAG_DROP reserved") !== 0)
    throw new YasProtocolError("Selection DRAG_DROP reserved field is nonzero");
  const value = {
    dragHandle,
    revision,
    operationId,
    selectedAction,
    extensions: decodeExtensions(
      cursor,
      new Set([YAS_SELECTION_DRAG_DROP_ITEMS_EXTENSION]),
      "Selection DRAG_DROP extensions",
    ),
  };
  cursor.end("Selection DRAG_DROP");
  encodeSelectionDragDrop(value);
  return value;
}

export interface YasSelectionSlotRecord {
  kind: "slot";
  slot: number;
  ownerKind: number;
  ownerHandle: bigint;
  revision: bigint;
  mimeTypes: readonly string[];
  extensions: readonly YasExtension[];
}

export interface YasSelectionDragRecord {
  kind: "drag";
  dragHandle: bigint;
  revision: bigint;
  ownerSession: Uint8Array;
  sourceActions: number;
  selectedAction: number;
  targetSurface: bigint;
  items: readonly YasSelectionDragItem[];
  extensions: readonly YasExtension[];
}

export interface YasSelectionSnapshot {
  revision: bigint;
  slots: readonly YasSelectionSlotRecord[];
  drags: readonly YasSelectionDragRecord[];
}

export function encodeSelectionSet(
  slot: number,
  operationId: Uint8Array,
  items: readonly YasSelectionInlineItem[],
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  validateSlot(slot);
  requireOperationId(operationId, "Selection SET");
  if (items.length === 0 || items.length > YAS_SELECTION_MAX_ITEMS)
    throw new YasProtocolError("invalid Selection SET item count");
  const writer = new YasWriter()
    .u8(slot)
    .bytes(new Uint8Array(3))
    .bytes(operationId)
    .u16(items.length);
  let encodedBytes = 0;
  validateOrderedMimes(
    items.map((item) => item.mime),
    false,
  );
  for (const item of items) {
    const mimeLength = utf8Length(item.mime);
    encodedBytes += 2 + mimeLength + 4 + item.data.length;
    if (encodedBytes > YAS_SELECTION_MAX_INLINE_BYTES)
      throw new YasProtocolError("Selection inline items exceed their limit");
    writer.utf8U16(item.mime).bytesU32(item.data);
  }
  return writer.bytes(encodeExtensions(extensions)).finish();
}

export function encodeSelectionSetBegin(
  slot: number,
  operationId: Uint8Array,
  items: readonly YasSelectionUploadItem[],
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  validateSlot(slot);
  requireOperationId(operationId, "Selection SET_BEGIN");
  if (items.length === 0 || items.length > YAS_SELECTION_MAX_ITEMS)
    throw new YasProtocolError("invalid Selection SET_BEGIN item count");
  validateOrderedMimes(
    items.map((item) => item.mime),
    false,
  );
  const writer = new YasWriter()
    .u8(slot)
    .bytes(new Uint8Array(3))
    .bytes(operationId)
    .u16(items.length);
  for (const item of items) {
    requireHash(item.contentHash);
    writer
      .utf8U16(item.mime)
      .u64(item.byteLength)
      .bytes(item.contentHash)
      .u64(item.initialReceiveCredit);
  }
  return writer.bytes(encodeExtensions(extensions)).finish();
}

export function decodeSelectionSetBeginResult(bytes: Uint8Array): {
  stagingHandle: bigint;
  descriptors: readonly YasTransferDescriptor[];
  extensions: readonly YasExtension[];
} {
  const cursor = new YasCursor(bytes);
  const stagingHandle = cursor.u64("Selection staging handle");
  const count = cursor.u16("Selection upload descriptor count");
  if (
    cursor.u16("Selection upload reserved") !== 0 ||
    count === 0 ||
    count > YAS_SELECTION_MAX_ITEMS ||
    count > Math.floor(cursor.remaining / 4)
  )
    throw new YasProtocolError("invalid Selection upload descriptor count");
  const descriptors: YasTransferDescriptor[] = [];
  const ids = new Set<number>();
  for (let index = 0; index < count; index++) {
    const descriptorCursor = cursor.sub(
      cursor.u32("Selection Transfer descriptor length"),
      "Selection Transfer descriptor",
    );
    const descriptor = decodeTransferDescriptor(descriptorCursor);
    descriptorCursor.end("Selection Transfer descriptor");
    validateItemTransfer(descriptor, YAS_TRANSFER_RECEIVER_TO_SENDER);
    requireTransferUploadStage(
      descriptor,
      stagingHandle,
      "Selection item descriptor",
    );
    if (ids.has(descriptor.transferId))
      throw new YasProtocolError("duplicate Selection Transfer ID");
    ids.add(descriptor.transferId);
    descriptors.push(descriptor);
  }
  const extensions = decodeExtensions(
    cursor,
    undefined,
    "Selection SET_BEGIN Result extensions",
  );
  cursor.end("Selection SET_BEGIN Result");
  requireHandle(stagingHandle, "Selection staging handle");
  return { stagingHandle, descriptors, extensions };
}

export function encodeSelectionSetCommit(
  stagingHandle: bigint,
  operationId: Uint8Array,
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  requireHandle(stagingHandle, "Selection staging handle");
  requireOperationId(operationId, "Selection SET_COMMIT");
  return new YasWriter()
    .u64(stagingHandle)
    .bytes(operationId)
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function encodeSelectionGet(value: YasSelectionGet): Uint8Array {
  const writer = new YasWriter();
  if (value.target.kind === "slot") {
    validateSlot(value.target.slot);
    requireRevision(value.target.revision, "Selection slot revision");
    writer
      .u8(YAS_SELECTION_GET_TARGET_SLOT)
      .bytes(new Uint8Array(3))
      .u8(value.target.slot)
      .bytes(new Uint8Array(3))
      .u64(value.target.revision);
  } else {
    requireHandle(value.target.dragHandle, "Selection drag handle");
    requireRevision(value.target.revision, "Selection drag revision");
    writer
      .u8(YAS_SELECTION_GET_TARGET_DRAG)
      .bytes(new Uint8Array(3))
      .u64(value.target.dragHandle)
      .u64(value.target.revision)
      .u16(value.target.itemIndex)
      .u16(0);
  }
  validateMime(value.mime);
  return writer
    .u64(value.initialReceiveCredit)
    .utf8U16(value.mime)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeSelectionGet(bytes: Uint8Array): YasSelectionGet {
  const cursor = new YasCursor(bytes);
  const targetKind = cursor.u8("Selection GET target");
  requireZero(cursor.take(3, "Selection GET reserved"), "Selection GET");
  let target: YasSelectionGetTarget;
  if (targetKind === YAS_SELECTION_GET_TARGET_SLOT) {
    const slot = cursor.u8("Selection slot");
    requireZero(
      cursor.take(3, "Selection slot target reserved"),
      "Selection slot target",
    );
    target = {
      kind: "slot",
      slot,
      revision: cursor.u64("Selection slot revision"),
    };
    validateSlot(target.slot);
  } else if (targetKind === YAS_SELECTION_GET_TARGET_DRAG) {
    target = {
      kind: "drag",
      dragHandle: cursor.u64("Selection drag handle"),
      revision: cursor.u64("Selection drag revision"),
      itemIndex: cursor.u16("Selection drag item index"),
    };
    if (cursor.u16("Selection drag target reserved") !== 0)
      throw new YasProtocolError("Selection drag target reserved is nonzero");
    requireHandle(target.dragHandle, "Selection drag handle");
  } else {
    throw new YasProtocolError("unknown Selection GET target");
  }
  requireRevision(target.revision, "Selection target revision");
  const value: YasSelectionGet = {
    target,
    initialReceiveCredit: cursor.u64("Selection initial receive credit"),
    mime: cursor.utf8U16("Selection MIME"),
    extensions: decodeExtensions(cursor, undefined, "Selection GET extensions"),
  };
  cursor.end("Selection GET");
  validateMime(value.mime);
  return value;
}

export function encodeSelectionDragPosition(
  value: YasSelectionDragPosition,
): Uint8Array {
  validateDragPosition(value);
  return new YasWriter()
    .u64(value.dragHandle)
    .u64(value.revision)
    .u64(value.targetSurface)
    .i64(value.x32_32)
    .i64(value.y32_32)
    .u16(value.actions)
    .u16(0)
    .finish();
}

export function decodeSelectionDragPosition(
  bytes: Uint8Array,
): YasSelectionDragPosition {
  const cursor = new YasCursor(bytes);
  const value: YasSelectionDragPosition = {
    dragHandle: cursor.u64("Selection drag handle"),
    revision: cursor.u64("Selection drag revision"),
    targetSurface: cursor.u64("Selection target surface"),
    x32_32: cursor.i64("Selection drag x"),
    y32_32: cursor.i64("Selection drag y"),
    actions: cursor.u16("Selection drag actions"),
  };
  if (cursor.u16("Selection drag position reserved") !== 0)
    throw new YasProtocolError("Selection drag position reserved is nonzero");
  cursor.end("Selection drag position");
  validateDragPosition(value);
  return value;
}

export function encodeSelectionDragLeave(
  value: YasSelectionDragLeave,
): Uint8Array {
  validateDragIdentity(value);
  return new YasWriter()
    .u64(value.dragHandle)
    .u64(value.revision)
    .u64(value.targetSurface)
    .finish();
}

export function decodeSelectionDragLeave(
  bytes: Uint8Array,
): YasSelectionDragLeave {
  const cursor = new YasCursor(bytes);
  const value = {
    dragHandle: cursor.u64("Selection drag handle"),
    revision: cursor.u64("Selection drag revision"),
    targetSurface: cursor.u64("Selection target surface"),
  };
  cursor.end("Selection DRAG_LEAVE");
  validateDragIdentity(value);
  return value;
}

export function decodeSelectionSlotRecord(
  bytes: Uint8Array,
): YasSelectionSlotRecord {
  const cursor = new YasCursor(bytes);
  const slot = cursor.u8("Selection slot");
  const ownerKind = cursor.u8("Selection owner kind");
  if (cursor.u16("Selection record reserved") !== 0)
    throw new YasProtocolError("Selection record reserved is nonzero");
  const value: YasSelectionSlotRecord = {
    kind: "slot",
    slot,
    ownerKind,
    ownerHandle: cursor.u64("Selection owner handle"),
    revision: cursor.u64("Selection revision"),
    mimeTypes: decodeMimes(cursor, true),
    extensions: decodeExtensions(cursor, undefined, "Selection extensions"),
  };
  cursor.end("Selection slot record");
  validateSlot(value.slot);
  requireRevision(value.revision, "Selection revision");
  if (
    value.ownerKind > YAS_SELECTION_OWNER_EXTERNAL ||
    (value.ownerKind === YAS_SELECTION_OWNER_NONE) !==
      (value.ownerHandle === 0n) ||
    (value.ownerKind === YAS_SELECTION_OWNER_NONE &&
      value.mimeTypes.length !== 0)
  )
    throw new YasProtocolError("invalid Selection owner");
  return value;
}

export function decodeSelectionDragRecord(
  bytes: Uint8Array,
): YasSelectionDragRecord {
  const cursor = new YasCursor(bytes);
  const value: YasSelectionDragRecord = {
    kind: "drag",
    dragHandle: cursor.u64("Selection drag handle"),
    revision: cursor.u64("Selection drag revision"),
    ownerSession: new Uint8Array(cursor.take(16, "Selection owner session")),
    sourceActions: cursor.u16("Selection source actions"),
    selectedAction: cursor.u16("Selection selected action"),
    targetSurface: cursor.u64("Selection target surface"),
    items: decodeDragItems(cursor),
    extensions: decodeExtensions(
      cursor,
      undefined,
      "Selection drag extensions",
    ),
  };
  cursor.end("Selection drag record");
  requireHandle(value.dragHandle, "Selection drag handle");
  requireRevision(value.revision, "Selection drag revision");
  validateActions(value.sourceActions, false, false);
  validateActions(value.selectedAction, true, true);
  if (value.selectedAction & ~value.sourceActions)
    throw new YasProtocolError(
      "Selection action was not offered by the source",
    );
  return value;
}

function selectionSlotRetentionKey(slot: number): string {
  return `slot:${slot}`;
}

function selectionDragRetentionKey(dragHandle: bigint): string {
  return `drag:${dragHandle}`;
}

function encodeSelectionSlotStateRecord(
  value: YasSelectionSlotRecord,
): Uint8Array {
  const writer = new YasWriter()
    .u16(YAS_SELECTION_ENTITY_SLOT)
    .u16(0)
    .u8(value.slot)
    .u8(value.ownerKind)
    .u16(0)
    .u64(value.ownerHandle)
    .u64(value.revision)
    .u16(value.mimeTypes.length);
  for (const mime of value.mimeTypes) writer.utf8U16(mime);
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

function encodeSelectionDragStateRecord(
  value: YasSelectionDragRecord,
): Uint8Array {
  const writer = new YasWriter()
    .u16(YAS_SELECTION_ENTITY_DRAG)
    .u16(0)
    .u64(value.dragHandle)
    .u64(value.revision)
    .bytes(value.ownerSession)
    .u16(value.sourceActions)
    .u16(value.selectedAction)
    .u64(value.targetSurface)
    .u16(value.items.length);
  encodeDragItems(writer, value.items);
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

function selectionSlotRetainedBytes(value: YasSelectionSlotRecord): number {
  return Math.max(
    encodeSelectionSlotStateRecord(value).length,
    estimateStateRetainedBytes(value),
  );
}

function selectionDragRetainedBytes(value: YasSelectionDragRecord): number {
  return Math.max(
    encodeSelectionDragStateRecord(value).length,
    estimateStateRetainedBytes(value),
  );
}

function detachSelectionSlotRecord(
  value: YasSelectionSlotRecord,
): YasSelectionSlotRecord {
  const encoded = encodeSelectionSlotStateRecord(value);
  return decodeSelectionSlotRecord(encoded.subarray(4));
}

function detachSelectionDragRecord(
  value: YasSelectionDragRecord,
): YasSelectionDragRecord {
  const encoded = encodeSelectionDragStateRecord(value);
  return decodeSelectionDragRecord(encoded.subarray(4));
}

interface YasSelectionCatalogLimits {
  maxActiveDrags: number;
  maxItems: number;
}

export class YasSelectionCatalog {
  private slots = new Map<number, YasSelectionSlotRecord>();
  private drags = new Map<bigint, YasSelectionDragRecord>();
  private currentRetention: YasStateCatalogueRetention<string>;
  private staging: {
    slots: Map<number, YasSelectionSlotRecord>;
    drags: Map<bigint, YasSelectionDragRecord>;
  } | null = null;
  private stagingRetention: YasStateCatalogueRetention<string> | null = null;
  private subscription: YasStateSubscription | null = null;
  private listeners = new Set<(snapshot: YasSelectionSnapshot) => void>();
  private pendingFirstSnapshots = new Set<(error: unknown) => void>();
  private revision = 0n;
  private readonly removeInvalidation: () => void;
  private pendingWatch: Promise<void> | null = null;
  private pendingWatchCancel: ((error: unknown) => void) | null = null;
  private watchEpoch = 0;
  private disposed = false;

  constructor(
    private readonly connection: YasConnection,
    private readonly limits: () => YasSelectionCatalogLimits = () => ({
      maxActiveDrags: YAS_SELECTION_MAX_ACTIVE_DRAGS_PER_SESSION,
      maxItems: YAS_SELECTION_MAX_ITEMS,
    }),
  ) {
    this.currentRetention =
      YasStateCatalogueRetention.forConnection(connection);
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family === undefined || family === YAS_FAMILY_SELECTION) {
        this.cancelPendingWatch(
          new YasProtocolError("Selection catalogue was invalidated"),
        );
        this.resetLocal();
      }
    });
  }

  get snapshot(): YasSelectionSnapshot {
    return {
      revision: this.revision,
      slots: [...this.slots.values()],
      drags: [...this.drags.values()],
    };
  }

  subscribe(listener: (snapshot: YasSelectionSnapshot) => void): () => void {
    if (this.disposed) throw new Error("Selection catalogue is disposed");
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
  ): Promise<YasSelectionSnapshot> {
    if (this.disposed) throw new Error("Selection catalogue is disposed");
    if (this.revision !== 0n && this.subscription?.active) return this.snapshot;
    let remove: (() => void) | undefined;
    let rejectPending!: (error: unknown) => void;
    const result = new Promise<YasSelectionSnapshot>((resolve, reject) => {
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
      return Promise.reject(new Error("Selection catalogue is disposed"));
    if (this.subscription?.active) return Promise.resolve();
    if (this.pendingWatch) return this.pendingWatch;
    this.resetLocal();
    const epoch = this.watchEpoch;
    const watched = YasStateSubscription.watch(
      this.connection,
      YAS_FAMILY_SELECTION,
      YAS_SELECTION_WATCH,
      YAS_SELECTION_UNWATCH,
      YAS_SELECTION_STATE,
      YAS_SELECTION_STATE_ACK,
      options,
      (batch) => {
        if (!this.disposed && epoch === this.watchEpoch) this.apply(batch);
      },
    ).then(async (subscription) => {
      if (this.disposed || epoch !== this.watchEpoch) {
        await subscription.unwatch().catch(() => undefined);
        throw new YasProtocolError("Selection catalogue watch was cancelled");
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
      new YasProtocolError("Selection catalogue watch was cancelled"),
    );
    const subscription = this.subscription;
    this.subscription = null;
    if (!this.disposed) this.clearState();
    await subscription?.unwatch();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    const disposalError = new Error("Selection catalogue is disposed");
    this.cancelPendingWatch(disposalError);
    this.removeInvalidation();
    for (const reject of [...this.pendingFirstSnapshots]) reject(disposalError);
    this.pendingFirstSnapshots.clear();
    this.listeners.clear();
    const subscription = this.subscription;
    this.subscription = null;
    this.currentRetention.dispose();
    this.stagingRetention?.dispose();
    this.slots.clear();
    this.drags.clear();
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
      this.stagingRetention?.dispose();
      this.staging = { slots: new Map(), drags: new Map() };
      this.stagingRetention = YasStateCatalogueRetention.forConnection(
        this.connection,
      );
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_RECORDS) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("Selection snapshot records without begin");
      try {
        this.applyRecords(
          this.staging.slots,
          this.staging.drags,
          this.stagingRetention,
          batch.records,
        );
      } catch (error) {
        this.discardStaging();
        throw error;
      }
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_END) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("Selection snapshot end without begin");
      try {
        this.applyRecords(
          this.staging.slots,
          this.staging.drags,
          this.stagingRetention,
          batch.records,
        );
      } catch (error) {
        this.discardStaging();
        throw error;
      }
      const previousRetention = this.currentRetention;
      this.slots = this.staging.slots;
      this.drags = this.staging.drags;
      this.currentRetention = this.stagingRetention;
      this.staging = null;
      this.stagingRetention = null;
      previousRetention.dispose();
      this.revision = batch.toRevision;
      this.emit();
      return;
    }
    if (batch.phase === YAS_STATE_DELTA) {
      const retention = this.currentRetention.clone();
      let slots: Map<number, YasSelectionSlotRecord>;
      let drags: Map<bigint, YasSelectionDragRecord>;
      try {
        slots = new Map(this.slots);
        drags = new Map(this.drags);
        this.applyRecords(slots, drags, retention, batch.records);
      } catch (error) {
        retention.dispose();
        throw error;
      }
      const previousRetention = this.currentRetention;
      this.slots = slots;
      this.drags = drags;
      this.currentRetention = retention;
      previousRetention.dispose();
      this.revision = batch.toRevision;
      this.emit();
    }
  }

  private validateCatalog(
    drags: ReadonlyMap<bigint, YasSelectionDragRecord>,
  ): void {
    const limits = this.limits();
    if (drags.size > limits.maxActiveDrags)
      throw new YasProtocolError(
        "Selection catalogue exceeds negotiated active-drag limit",
      );
    for (const drag of drags.values())
      if (drag.items.length > limits.maxItems)
        throw new YasProtocolError(
          "Selection drag exceeds negotiated item limit",
        );
  }

  private applyRecords(
    slots: Map<number, YasSelectionSlotRecord>,
    drags: Map<bigint, YasSelectionDragRecord>,
    retention: YasStateCatalogueRetention<string>,
    records: readonly YasTypedRecord[],
  ): void {
    const originalSlots = new Map<number, YasSelectionSlotRecord | null>();
    const originalDrags = new Map<bigint, YasSelectionDragRecord | null>();
    const rememberSlot = (slot: number) => {
      if (!originalSlots.has(slot))
        originalSlots.set(slot, slots.get(slot) ?? null);
    };
    const rememberDrag = (dragHandle: bigint) => {
      if (!originalDrags.has(dragHandle))
        originalDrags.set(dragHandle, drags.get(dragHandle) ?? null);
    };
    try {
      for (const action of records) {
        const cursor = new YasCursor(action.body);
        const entity = cursor.u16("Selection state entity");
        if (cursor.u16("Selection state reserved") !== 0)
          throw new YasProtocolError("Selection state reserved is nonzero");
        const payload = new Uint8Array(cursor.take(cursor.remaining));
        if (entity === YAS_SELECTION_ENTITY_SLOT)
          this.applySlot(slots, retention, rememberSlot, action.kind, payload);
        else if (entity === YAS_SELECTION_ENTITY_DRAG)
          this.applyDrag(drags, retention, rememberDrag, action.kind, payload);
        else throw new YasProtocolError("unknown Selection state entity");
      }
      this.validateCatalog(drags);
    } catch (error) {
      for (const slot of originalSlots.keys())
        retention.remove(selectionSlotRetentionKey(slot));
      for (const dragHandle of originalDrags.keys())
        retention.remove(selectionDragRetentionKey(dragHandle));
      for (const [slot, original] of originalSlots) {
        if (original) {
          retention.upsert(
            selectionSlotRetentionKey(slot),
            selectionSlotRetainedBytes(original),
          );
          slots.set(slot, original);
        } else slots.delete(slot);
      }
      for (const [dragHandle, original] of originalDrags) {
        if (original) {
          retention.upsert(
            selectionDragRetentionKey(dragHandle),
            selectionDragRetainedBytes(original),
          );
          drags.set(dragHandle, original);
        } else drags.delete(dragHandle);
      }
      throw error;
    }
  }

  private applySlot(
    target: Map<number, YasSelectionSlotRecord>,
    retention: YasStateCatalogueRetention<string>,
    remember: (slot: number) => void,
    action: number,
    payload: Uint8Array,
  ): void {
    if (action === YAS_STATE_ADD || action === YAS_STATE_REPLACE) {
      const value = detachSelectionSlotRecord(
        decodeSelectionSlotRecord(payload),
      );
      const exists = target.has(value.slot);
      if ((action === YAS_STATE_ADD) === exists)
        throw new YasProtocolError(
          "Selection slot ADD/REPLACE precondition failed",
        );
      remember(value.slot);
      retention.upsert(
        selectionSlotRetentionKey(value.slot),
        selectionSlotRetainedBytes(value),
      );
      target.set(value.slot, value);
      return;
    }
    const cursor = new YasCursor(payload);
    const slot = cursor.u8("Selection slot state key");
    requireZero(
      cursor.take(3, "Selection slot state reserved"),
      "Selection slot state",
    );
    const revision = cursor.u64("Selection slot state revision");
    validateSlot(slot);
    requireRevision(revision, "Selection slot state revision");
    if (action === YAS_STATE_PATCH) {
      const extensions = decodeExtensions(
        cursor,
        undefined,
        "Selection slot patch",
      );
      cursor.end("Selection slot PATCH");
      const previous = target.get(slot);
      if (!previous)
        throw new YasProtocolError(
          "Selection slot PATCH names an unknown slot",
        );
      const value = detachSelectionSlotRecord({
        ...previous,
        revision,
        extensions: mergeExtensions(previous.extensions, extensions),
      });
      remember(slot);
      retention.upsert(
        selectionSlotRetentionKey(slot),
        selectionSlotRetainedBytes(value),
      );
      target.set(slot, value);
    } else if (action === YAS_STATE_REMOVE) {
      cursor.end("Selection slot REMOVE");
      if (!target.has(slot))
        throw new YasProtocolError(
          "Selection slot REMOVE names an unknown slot",
        );
      remember(slot);
      retention.remove(selectionSlotRetentionKey(slot));
      target.delete(slot);
    } else {
      throw new YasProtocolError(
        "unsupported Selection slot state record kind",
      );
    }
  }

  private applyDrag(
    target: Map<bigint, YasSelectionDragRecord>,
    retention: YasStateCatalogueRetention<string>,
    remember: (dragHandle: bigint) => void,
    action: number,
    payload: Uint8Array,
  ): void {
    if (action === YAS_STATE_ADD || action === YAS_STATE_REPLACE) {
      const value = detachSelectionDragRecord(
        decodeSelectionDragRecord(payload),
      );
      const exists = target.has(value.dragHandle);
      if ((action === YAS_STATE_ADD) === exists)
        throw new YasProtocolError(
          "Selection drag ADD/REPLACE precondition failed",
        );
      remember(value.dragHandle);
      retention.upsert(
        selectionDragRetentionKey(value.dragHandle),
        selectionDragRetainedBytes(value),
      );
      target.set(value.dragHandle, value);
      return;
    }
    const cursor = new YasCursor(payload);
    const dragHandle = cursor.u64("Selection drag state key");
    const revision = cursor.u64("Selection drag state revision");
    requireHandle(dragHandle, "Selection drag state key");
    requireRevision(revision, "Selection drag state revision");
    if (action === YAS_STATE_PATCH) {
      const extensions = decodeExtensions(
        cursor,
        undefined,
        "Selection drag patch",
      );
      cursor.end("Selection drag PATCH");
      const previous = target.get(dragHandle);
      if (!previous)
        throw new YasProtocolError(
          "Selection drag PATCH names an unknown drag",
        );
      const value = detachSelectionDragRecord({
        ...previous,
        revision,
        extensions: mergeExtensions(previous.extensions, extensions),
      });
      remember(dragHandle);
      retention.upsert(
        selectionDragRetentionKey(dragHandle),
        selectionDragRetainedBytes(value),
      );
      target.set(dragHandle, value);
    } else if (action === YAS_STATE_REMOVE) {
      cursor.end("Selection drag REMOVE");
      if (!target.has(dragHandle))
        throw new YasProtocolError(
          "Selection drag REMOVE names an unknown drag",
        );
      remember(dragHandle);
      retention.remove(selectionDragRetentionKey(dragHandle));
      target.delete(dragHandle);
    } else {
      throw new YasProtocolError(
        "unsupported Selection drag state record kind",
      );
    }
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
    this.currentRetention.dispose();
    this.stagingRetention?.dispose();
    this.slots = new Map();
    this.drags = new Map();
    this.currentRetention = YasStateCatalogueRetention.forConnection(
      this.connection,
    );
    this.staging = null;
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

export class YasSelectionClient {
  readonly catalog: YasSelectionCatalog;
  private readonly transfers;
  private readonly beginOperations = new Map<
    string,
    YasSelectionBeginOperation
  >();
  private readonly pendingBeginOperations = new Map<
    string,
    YasSelectionBeginOperation
  >();
  private readonly stages = new Map<bigint, YasSelectionStageOwnership>();
  private readonly pendingCancels = new Set<(error: unknown) => void>();
  private removeListeners: (() => void)[];
  private enterListeners = new Set<(event: YasSelectionDragPosition) => void>();
  private motionListeners = new Set<
    (event: YasSelectionDragPosition) => void
  >();
  private leaveListeners = new Set<(event: YasSelectionDragLeave) => void>();
  private generation = 0;
  private disposed = false;

  constructor(readonly connection: YasConnection) {
    this.catalog = new YasSelectionCatalog(connection, () => ({
      maxActiveDrags: negotiatedStateLimitU32(
        connection,
        YAS_FAMILY_SELECTION,
        YAS_SELECTION_VERSION,
        YAS_SELECTION_LIMIT_MAX_ACTIVE_DRAGS_PER_SESSION,
        YAS_SELECTION_MAX_ACTIVE_DRAGS_PER_SESSION,
      ),
      maxItems: negotiatedStateLimitU32(
        connection,
        YAS_FAMILY_SELECTION,
        YAS_SELECTION_VERSION,
        YAS_SELECTION_LIMIT_MAX_ITEMS,
        YAS_SELECTION_MAX_ITEMS,
      ),
    }));
    this.transfers = transfersFor(connection);
    this.removeListeners = [
      connection.onEvent(
        YAS_FAMILY_SELECTION,
        YAS_SELECTION_DRAG_ENTER,
        ({ payload }) =>
          this.emit(this.enterListeners, decodeSelectionDragPosition(payload)),
      ),
      connection.onEvent(
        YAS_FAMILY_SELECTION,
        YAS_SELECTION_DRAG_MOTION,
        ({ payload }) =>
          this.emit(this.motionListeners, decodeSelectionDragPosition(payload)),
      ),
      connection.onEvent(
        YAS_FAMILY_SELECTION,
        YAS_SELECTION_DRAG_LEAVE,
        ({ payload }) =>
          this.emit(this.leaveListeners, decodeSelectionDragLeave(payload)),
      ),
      connection.onInvalidation(({ family }) => {
        if (family !== undefined && family !== YAS_FAMILY_SELECTION) return;
        this.generation++;
        this.retirePendingBeginOperations();
        const error = new YasProtocolError(
          "YAS Selection client was invalidated",
        );
        for (const cancel of [...this.pendingCancels]) cancel(error);
        this.pendingCancels.clear();
        this.retireAllStages(true);
      }),
    ];
  }

  list(options: YasWatchOptions = {}): Promise<YasSelectionSnapshot> {
    return this.catalog.firstSnapshot(options);
  }

  set(
    slot: number,
    operationId: Uint8Array,
    items: readonly YasSelectionInlineItem[],
    extensions: readonly YasExtension[] = [],
  ): Promise<bigint> {
    return this.revisionRequest(
      YAS_SELECTION_SET,
      encodeSelectionSet(slot, operationId, items, extensions),
    );
  }

  beginSet(
    slot: number,
    operationId: Uint8Array,
    items: readonly YasSelectionUploadItem[],
    extensions: readonly YasExtension[] = [],
  ): Promise<YasSelectionUploadBatch> {
    this.assertOpen();
    this.pruneExpiredStages();
    const payload = encodeSelectionSetBegin(
      slot,
      operationId,
      items,
      extensions,
    );
    const operationKey = byteKey(operationId);
    const payloadKey = byteKey(payload);
    let operation =
      this.beginOperations.get(operationKey) ??
      this.pendingBeginOperations.get(operationKey);
    if (operation) {
      if (operation.payloadKey !== payloadKey)
        throw new YasProtocolError(
          "Selection SET_BEGIN operation ID was reused with a different payload",
        );
      if (operation.pending) return operation.pending;
      if (
        operation.batch &&
        this.stages.get(operation.batch.stagingHandle)?.batch ===
          operation.batch
      )
        return Promise.resolve(operation.batch);
      operation.batch = null;
      if (!this.canReserveBeginOperation(operationKey))
        return Promise.reject(
          new YasProtocolError(
            "Selection SET_BEGIN replay ledger has no evictable settlement",
          ),
        );
      return this.startBeginOperation(
        operationKey,
        operation,
        payload,
        items.length,
        false,
      );
    } else {
      if (!this.canReserveBeginOperation(operationKey))
        return Promise.reject(
          new YasProtocolError(
            "Selection SET_BEGIN replay ledger has no evictable settlement",
          ),
        );
      operation = {
        payloadKey,
        pending: null,
        batch: null,
        identity: null,
        settled: false,
      };
      this.pendingBeginOperations.set(operationKey, operation);
    }
    return this.startBeginOperation(
      operationKey,
      operation,
      payload,
      items.length,
      true,
    );
  }

  private startBeginOperation(
    operationKey: string,
    operation: YasSelectionBeginOperation,
    payload: Uint8Array,
    itemCount: number,
    fresh: boolean,
  ): Promise<YasSelectionUploadBatch> {
    const generation = this.generation;
    const running = fresh
      ? this.performFreshBeginSet(payload, itemCount, generation, operation)
      : this.performRetiredBeginSet(payload, itemCount, operation);
    let pending!: Promise<YasSelectionUploadBatch>;
    pending = this.runOwned(running)
      .then((batch) => {
        if (!fresh) {
          this.discardUnexpectedBatch(batch, operation.identity);
          throw new YasProtocolError(
            "retired Selection SET_BEGIN unexpectedly returned OK",
          );
        }
        if (this.disposed || generation !== this.generation) {
          this.discardUnexpectedBatch(batch, operation.identity);
          throw new YasProtocolError(
            "Selection SET_BEGIN completed after family invalidation",
          );
        }
        if (!this.retainBeginOperation(operationKey, operation)) {
          this.discardUnexpectedBatch(batch, operation.identity);
          throw new YasProtocolError(
            "Selection SET_BEGIN replay ledger has no evictable settlement",
          );
        }
        operation.batch = batch;
        operation.identity = selectionBeginIdentity(batch);
        operation.settled = true;
        try {
          this.trackStage(operationKey, batch);
        } catch (error) {
          if (operation.batch === batch) operation.batch = null;
          this.discardUnexpectedBatch(batch, null);
          throw error;
        }
        return batch;
      })
      .finally(() => {
        if (operation.pending !== pending) return;
        operation.pending = null;
        if (
          fresh &&
          this.pendingBeginOperations.get(operationKey) === operation
        )
          this.pendingBeginOperations.delete(operationKey);
      });
    operation.pending = pending;
    return pending;
  }

  async commitSet(
    stagingHandle: bigint,
    operationId: Uint8Array,
    extensions: readonly YasExtension[] = [],
  ): Promise<bigint> {
    this.assertOpen();
    this.pruneExpiredStages();
    try {
      const revision = await this.revisionRequest(
        YAS_SELECTION_SET_COMMIT,
        encodeSelectionSetCommit(stagingHandle, operationId, extensions),
      );
      this.retireStage(stagingHandle, false);
      return revision;
    } catch (error) {
      if (
        error instanceof YasResultError &&
        error.status === YAS_STATUS_NOT_FOUND
      )
        this.retireStage(stagingHandle, false);
      throw error;
    }
  }

  async get(
    value: Omit<YasSelectionGet, "initialReceiveCredit"> & {
      initialReceiveCredit?: bigint;
    },
  ): Promise<YasSelectionContent> {
    const lease = this.transfers.reserveReceiveCredit(
      value.initialReceiveCredit ?? 1024n * 1024n,
      1024n,
    );
    let accepted = false;
    try {
      return await this.connection.requestDecoded(
        YAS_FAMILY_SELECTION,
        YAS_SELECTION_GET,
        encodeSelectionGet({ ...value, initialReceiveCredit: lease.bytes }),
        (body) => {
          const delivery = decodeInlineOrTransfer(body);
          if (delivery.delivery === "inline") {
            if (delivery.bytes.length > YAS_SELECTION_MAX_INLINE_BYTES)
              throw new YasProtocolError(
                "Selection inline result exceeds its limit",
              );
            lease.release();
            accepted = true;
            const copy = new Uint8Array(delivery.bytes);
            return {
              byteLength: delivery.byteLength,
              contentHash: delivery.contentHash,
              bytes: async () => new Uint8Array(copy),
            };
          }
          validateItemTransfer(
            delivery.descriptor,
            YAS_TRANSFER_SENDER_TO_RECEIVER,
          );
          const transfer = this.transfers.acceptServerDescriptor(
            delivery.descriptor,
            lease,
          );
          accepted = true;
          let collected: Promise<Uint8Array> | undefined;
          return {
            byteLength: delivery.byteLength,
            contentHash: delivery.contentHash,
            bytes: () => (collected ??= transfer.collect(delivery.byteLength)),
          };
        },
      );
    } catch (error) {
      if (!accepted) lease.release();
      throw error;
    }
  }

  clear(
    slot: number,
    observedRevision: bigint,
    operationId: Uint8Array,
    extensions: readonly YasExtension[] = [],
  ): Promise<bigint> {
    validateSlot(slot);
    requireRevision(observedRevision, "Selection observed revision");
    requireOperationId(operationId, "Selection CLEAR");
    return this.revisionRequest(
      YAS_SELECTION_CLEAR,
      new YasWriter()
        .u8(slot)
        .bytes(new Uint8Array(3))
        .u64(observedRevision)
        .bytes(operationId)
        .bytes(encodeExtensions(extensions))
        .finish(),
    );
  }

  dragBegin(
    operationId: Uint8Array,
    sourceActions: number,
    items: readonly YasSelectionDragItem[],
    extensions: readonly YasExtension[] = [],
  ): Promise<{ dragHandle: bigint; revision: bigint }> {
    requireOperationId(operationId, "Selection DRAG_BEGIN");
    validateActions(sourceActions, false, false);
    validateDragItems(items);
    const writer = new YasWriter()
      .bytes(operationId)
      .u16(sourceActions)
      .u16(items.length);
    encodeDragItems(writer, items);
    return this.connection.requestDecoded(
      YAS_FAMILY_SELECTION,
      YAS_SELECTION_DRAG_BEGIN,
      writer.bytes(encodeExtensions(extensions)).finish(),
      (body) => {
        const cursor = new YasCursor(body);
        const result = {
          dragHandle: cursor.u64("Selection drag handle"),
          revision: cursor.u64("Selection drag revision"),
        };
        cursor.end("Selection DRAG_BEGIN Result");
        requireHandle(result.dragHandle, "Selection drag handle");
        requireRevision(result.revision, "Selection drag revision");
        return result;
      },
    );
  }

  async dragDrop(
    dragHandle: bigint,
    revision: bigint,
    operationId: Uint8Array,
    selectedAction: number,
    extensions: readonly YasExtension[] = [],
  ): Promise<void> {
    await this.connection.request(
      YAS_FAMILY_SELECTION,
      YAS_SELECTION_DRAG_DROP,
      encodeSelectionDragDrop({
        dragHandle,
        revision,
        operationId,
        selectedAction,
        extensions,
      }),
    );
  }

  async dragCancel(
    dragHandle: bigint,
    revision: bigint,
    operationId: Uint8Array,
    reason: string,
  ): Promise<void> {
    requireHandle(dragHandle, "Selection drag handle");
    requireRevision(revision, "Selection drag revision");
    requireOperationId(operationId, "Selection DRAG_CANCEL");
    await this.connection.request(
      YAS_FAMILY_SELECTION,
      YAS_SELECTION_DRAG_CANCEL,
      new YasWriter()
        .u64(dragHandle)
        .u64(revision)
        .bytes(operationId)
        .utf8U32(reason)
        .finish(),
    );
  }

  dragEnter(value: YasSelectionDragPosition): void {
    this.connection.sendEvent(
      YAS_FAMILY_SELECTION,
      YAS_SELECTION_DRAG_ENTER,
      encodeSelectionDragPosition(value),
    );
  }

  dragMotion(value: YasSelectionDragPosition): void {
    this.connection.sendEvent(
      YAS_FAMILY_SELECTION,
      YAS_SELECTION_DRAG_MOTION,
      encodeSelectionDragPosition(value),
    );
  }

  dragLeave(value: YasSelectionDragLeave): void {
    this.connection.sendEvent(
      YAS_FAMILY_SELECTION,
      YAS_SELECTION_DRAG_LEAVE,
      encodeSelectionDragLeave(value),
    );
  }

  onDragEnter(listener: (event: YasSelectionDragPosition) => void): () => void {
    this.enterListeners.add(listener);
    return () => this.enterListeners.delete(listener);
  }

  onDragMotion(
    listener: (event: YasSelectionDragPosition) => void,
  ): () => void {
    this.motionListeners.add(listener);
    return () => this.motionListeners.delete(listener);
  }

  onDragLeave(listener: (event: YasSelectionDragLeave) => void): () => void {
    this.leaveListeners.add(listener);
    return () => this.leaveListeners.delete(listener);
  }

  /** Register the client side of the bidirectional GET operation (inline v1). */
  handleInlineGet(
    handler: (request: YasSelectionGet) =>
      | Promise<{ bytes: Uint8Array; contentHash: Uint8Array }>
      | {
          bytes: Uint8Array;
          contentHash: Uint8Array;
        },
  ): () => void {
    return this.connection.handleRequests(
      YAS_FAMILY_SELECTION,
      YAS_SELECTION_GET,
      async ({ payload }) => {
        const value = await handler(decodeSelectionGet(payload));
        if (value.bytes.length > YAS_SELECTION_MAX_INLINE_BYTES)
          throw new YasProtocolError(
            "Selection inline response exceeds its limit",
          );
        requireHash(value.contentHash);
        return encodeInlineOrTransfer({
          delivery: "inline",
          byteLength: BigInt(value.bytes.length),
          contentHash: value.contentHash,
          bytes: value.bytes,
        });
      },
    );
  }

  /** Register the client side of GET, using an odd-ID Transfer when needed. */
  handleGet(
    handler: (request: YasSelectionGet) =>
      | Promise<{ bytes: Uint8Array; contentHash: Uint8Array }>
      | {
          bytes: Uint8Array;
          contentHash: Uint8Array;
        },
  ): () => void {
    return this.connection.handleRequests(
      YAS_FAMILY_SELECTION,
      YAS_SELECTION_GET,
      async ({ payload }) => {
        const request = decodeSelectionGet(payload);
        const value = await handler(request);
        requireHash(value.contentHash);
        if (value.bytes.length <= YAS_SELECTION_MAX_INLINE_BYTES)
          return encodeInlineOrTransfer({
            delivery: "inline",
            byteLength: BigInt(value.bytes.length),
            contentHash: value.contentHash,
            bytes: value.bytes,
          });
        if (request.initialReceiveCredit === 0n)
          throw new YasProtocolError(
            "Selection Transfer response has zero receive credit",
          );
        const sensitive: YasExtension = {
          tag: YAS_TRANSFER_SENSITIVE_CONTENT_EXTENSION,
          required: true,
          value: new Uint8Array(0),
        };
        const allocated = this.transfers.createClientDescriptor({
          mode: YAS_TRANSFER_MODE_BYTE,
          direction: YAS_TRANSFER_SENDER_TO_RECEIVER,
          receiverSendCredit: 0n,
          senderSendCredit: request.initialReceiveCredit,
          maxItemBytes: 0n,
          maxChunkBytes: this.transfers.maxOutboundChunkBytes(
            YAS_TRANSFER_MODE_BYTE,
          ),
          contentFamily: YAS_FAMILY_SELECTION,
          contentKind: YAS_SELECTION_ITEM_CONTENT_KIND,
          contentVersion: YAS_SELECTION_VERSION,
          extensions: [sensitive],
          sensitiveContent: true,
        });
        return {
          status: YAS_STATUS_OK,
          body: encodeInlineOrTransfer({
            delivery: "transfer",
            byteLength: BigInt(value.bytes.length),
            contentHash: value.contentHash,
            descriptor: allocated.descriptor,
          }),
          afterSend: async () => {
            try {
              await allocated.transfer.write(value.bytes);
              allocated.transfer.closeWrite();
            } catch (error) {
              allocated.transfer.reset(
                undefined,
                new TextEncoder().encode(
                  error instanceof Error ? error.message : String(error),
                ),
              );
            }
          },
        };
      },
    );
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.generation++;
    this.retirePendingBeginOperations();
    const error = new YasProtocolError("YAS Selection client was disposed");
    for (const cancel of [...this.pendingCancels]) cancel(error);
    this.pendingCancels.clear();
    this.retireAllStages(true);
    for (const remove of this.removeListeners) remove();
    this.removeListeners = [];
    this.beginOperations.clear();
    this.pendingBeginOperations.clear();
    this.enterListeners.clear();
    this.motionListeners.clear();
    this.leaveListeners.clear();
    this.catalog.dispose();
  }

  private performFreshBeginSet(
    payload: Uint8Array,
    itemCount: number,
    generation: number,
    operation: YasSelectionBeginOperation,
  ): Promise<YasSelectionUploadBatch> {
    return this.connection.requestDecoded<YasSelectionUploadBatch>(
      YAS_FAMILY_SELECTION,
      YAS_SELECTION_SET_BEGIN,
      payload,
      (body) => {
        const result = decodeSelectionSetBeginResult(body);
        if (this.disposed || generation !== this.generation) {
          this.discardUnexpectedBeginResult(
            result,
            itemCount,
            operation.identity,
          );
          throw new YasProtocolError(
            "Selection SET_BEGIN completed after family invalidation",
          );
        }
        if (this.beginResultHasOwnedAuthority(result, operation.identity))
          throw new YasProtocolError(
            "Selection SET_BEGIN returned an owned stage authority",
          );
        return this.acceptBeginResult(result, itemCount);
      },
    );
  }

  private performRetiredBeginSet(
    payload: Uint8Array,
    itemCount: number,
    operation: YasSelectionBeginOperation,
  ): Promise<YasSelectionUploadBatch> {
    return this.connection.requestDecoded<YasSelectionUploadBatch>(
      YAS_FAMILY_SELECTION,
      YAS_SELECTION_SET_BEGIN,
      payload,
      (body) => {
        const result = decodeSelectionSetBeginResult(body);
        this.discardUnexpectedBeginResult(
          result,
          itemCount,
          operation.identity,
        );
        throw new YasProtocolError(
          "retired Selection SET_BEGIN unexpectedly returned OK",
        );
      },
    );
  }

  private acceptBeginResult(
    result: ReturnType<typeof decodeSelectionSetBeginResult>,
    itemCount: number,
  ): YasSelectionUploadBatch {
    if (result.descriptors.length !== itemCount)
      throw new YasProtocolError("Selection upload descriptor count mismatch");
    const transfers: YasTransfer[] = [];
    try {
      for (const descriptor of result.descriptors)
        transfers.push(this.transfers.acceptServerUploadDescriptor(descriptor));
      const batch = {
        stagingHandle: result.stagingHandle,
        transfers,
        extensions: result.extensions,
      };
      selectionBatchExpiry(batch);
      return batch;
    } catch (error) {
      for (const transfer of transfers) transfer.reset();
      throw error;
    }
  }

  private replayLimit(): number {
    return negotiatedStateLimitU32(
      this.connection,
      YAS_FAMILY_SELECTION,
      YAS_SELECTION_VERSION,
      YAS_SELECTION_LIMIT_MAX_MUTATION_REPLAYS,
      YAS_SELECTION_MAX_MUTATION_REPLAYS,
    );
  }

  private canReserveBeginOperation(operationKey: string): boolean {
    const limit = this.replayLimit();
    let pinned = 0;
    for (const [key, operation] of this.beginOperations) {
      if (key === operationKey) continue;
      if (operation.pending || operation.batch) pinned++;
    }
    for (const key of this.pendingBeginOperations.keys())
      if (key !== operationKey) pinned++;
    return pinned + 1 <= limit;
  }

  private retainBeginOperation(
    operationKey: string,
    operation: YasSelectionBeginOperation,
  ): boolean {
    if (this.beginOperations.get(operationKey) === operation) return true;
    const limit = this.replayLimit();
    const needed = this.beginOperations.size - limit + 1;
    if (needed > 0) {
      const evictable = this.evictableBeginOperations();
      if (evictable.length < needed) return false;
      for (const key of evictable.slice(0, needed))
        this.beginOperations.delete(key);
    }
    this.pendingBeginOperations.delete(operationKey);
    this.beginOperations.set(operationKey, operation);
    return true;
  }

  private evictableBeginOperations(): string[] {
    const result: string[] = [];
    for (const [operationKey, operation] of this.beginOperations)
      if (operation.settled && !operation.pending && !operation.batch)
        result.push(operationKey);
    return result;
  }

  private beginResultHasOwnedAuthority(
    result: ReturnType<typeof decodeSelectionSetBeginResult>,
    identity: YasSelectionBeginIdentity | null,
  ): boolean {
    const transferIds = result.descriptors.map(
      (descriptor) => descriptor.transferId,
    );
    if (
      identity &&
      (identity.stagingHandle === result.stagingHandle ||
        transferIds.some((id) => identity.transferIds.includes(id)))
    )
      return true;
    for (const stage of this.stages.values()) {
      if (stage.batch.stagingHandle === result.stagingHandle) return true;
      const ownedIds = selectionBatchTransferIds(stage.batch);
      if (transferIds.some((id) => ownedIds.includes(id))) return true;
    }
    for (const operation of this.beginOperations.values()) {
      const owned = operation.identity;
      if (
        owned &&
        (owned.stagingHandle === result.stagingHandle ||
          transferIds.some((id) => owned.transferIds.includes(id)))
      )
        return true;
    }
    return transferIds.some((id) => this.transfers.get(id) !== undefined);
  }

  private discardUnexpectedBeginResult(
    result: ReturnType<typeof decodeSelectionSetBeginResult>,
    itemCount: number,
    identity: YasSelectionBeginIdentity | null,
  ): void {
    if (result.descriptors.length !== itemCount)
      throw new YasProtocolError("Selection upload descriptor count mismatch");
    if (this.beginResultHasOwnedAuthority(result, identity)) return;
    const batch = this.acceptBeginResult(result, itemCount);
    this.discardUnexpectedBatch(batch, identity);
  }

  private discardUnexpectedBatch(
    batch: YasSelectionUploadBatch,
    identity: YasSelectionBeginIdentity | null,
  ): void {
    if (
      identity?.stagingHandle === batch.stagingHandle ||
      this.stages.has(batch.stagingHandle) ||
      batch.transfers.some(
        (transfer) =>
          this.transfers.get(transfer.descriptor.transferId) !== transfer,
      )
    )
      return;
    this.resetBatch(batch);
  }

  private tombstoneBeginOperation(
    operationKey: string,
    batch: YasSelectionUploadBatch,
  ): void {
    const operation = this.beginOperations.get(operationKey);
    if (operation?.batch !== batch) return;
    operation.batch = null;
    operation.settled = true;
  }

  private trackStage(
    operationKey: string,
    batch: YasSelectionUploadBatch,
  ): void {
    if (this.stages.has(batch.stagingHandle))
      throw new YasProtocolError("Selection staging handle was reused");
    const expiresServerNs = selectionBatchExpiry(batch);
    const stage: YasSelectionStageOwnership = {
      operationKey,
      batch,
      expiresServerNs,
      removeListeners: [],
    };
    this.stages.set(batch.stagingHandle, stage);
    for (const transfer of batch.transfers) {
      stage.removeListeners.push(
        transfer.subscribeTerminal(() =>
          this.tombstoneBeginOperation(operationKey, batch),
        ),
      );
      stage.removeListeners.push(
        transfer.subscribeReset(() =>
          this.retireStage(batch.stagingHandle, false, batch),
        ),
      );
    }
    if (this.stages.get(batch.stagingHandle) !== stage)
      throw new YasProtocolError(
        "Selection SET_BEGIN completed with a retired stage",
      );
  }

  private pruneExpiredStages(): void {
    for (const [stagingHandle, stage] of this.stages)
      if (
        this.connection.nanosecondsUntilServerTime(stage.expiresServerNs) === 0n
      )
        this.retireStage(stagingHandle, true, stage.batch);
  }

  private retireStage(
    stagingHandle: bigint,
    reset: boolean,
    expectedBatch?: YasSelectionUploadBatch,
  ): void {
    const stage = this.stages.get(stagingHandle);
    if (!stage || (expectedBatch && stage.batch !== expectedBatch)) return;
    this.stages.delete(stagingHandle);
    for (const remove of stage.removeListeners) remove();
    stage.removeListeners = [];
    const operation = this.beginOperations.get(stage.operationKey);
    if (operation?.batch === stage.batch) {
      operation.batch = null;
      operation.settled = true;
    }
    if (reset) this.resetBatch(stage.batch);
  }

  private retireAllStages(reset: boolean): void {
    for (const [stagingHandle, stage] of [...this.stages])
      this.retireStage(stagingHandle, reset, stage.batch);
  }

  private retirePendingBeginOperations(): void {
    for (const operation of this.beginOperations.values()) {
      operation.pending = null;
      operation.batch = null;
      operation.settled = true;
    }
    for (const [operationKey, operation] of this.pendingBeginOperations) {
      operation.pending = null;
      operation.batch = null;
      operation.settled = true;
      this.retainBeginOperation(operationKey, operation);
    }
    this.pendingBeginOperations.clear();
  }

  private resetBatch(batch: YasSelectionUploadBatch): void {
    for (const transfer of batch.transfers) {
      try {
        transfer.reset();
      } catch {
        // Family invalidation may make Transfer cleanup unavailable.
      }
    }
  }

  private runOwned<T>(operation: Promise<T>): Promise<T> {
    let cancel!: (error: unknown) => void;
    const cancelled = new Promise<never>((_, reject) => {
      cancel = reject;
    });
    this.pendingCancels.add(cancel);
    return Promise.race([operation, cancelled]).finally(() => {
      this.pendingCancels.delete(cancel);
    });
  }

  private assertOpen(): void {
    if (this.disposed)
      throw new YasProtocolError("Selection client is disposed");
  }

  private revisionRequest(kind: number, payload: Uint8Array): Promise<bigint> {
    return this.connection.requestDecoded(
      YAS_FAMILY_SELECTION,
      kind,
      payload,
      (body) => {
        const cursor = new YasCursor(body);
        const revision = cursor.u64("Selection revision Result");
        cursor.end("Selection revision Result");
        requireRevision(revision, "Selection revision Result");
        return revision;
      },
    );
  }

  private emit<T>(listeners: ReadonlySet<(event: T) => void>, event: T): void {
    for (const listener of listeners) listener(event);
  }
}

function validateItemTransfer(
  descriptor: YasTransferDescriptor,
  direction: number,
): void {
  if (
    descriptor.mode !== YAS_TRANSFER_MODE_BYTE ||
    descriptor.direction !== direction ||
    descriptor.contentFamily !== YAS_FAMILY_SELECTION ||
    descriptor.contentKind !== YAS_SELECTION_ITEM_CONTENT_KIND ||
    descriptor.contentVersion !== YAS_SELECTION_VERSION ||
    !descriptor.sensitiveContent
  )
    throw new YasProtocolError("invalid Selection item Transfer descriptor");
}

function byteKey(value: Uint8Array): string {
  let output = "";
  for (const byte of value) output += String.fromCharCode(byte);
  return output;
}

function selectionBatchTransferIds(
  batch: YasSelectionUploadBatch,
): readonly number[] {
  return batch.transfers.map((transfer) => transfer.descriptor.transferId);
}

function selectionBeginIdentity(
  batch: YasSelectionUploadBatch,
): YasSelectionBeginIdentity {
  return {
    stagingHandle: batch.stagingHandle,
    transferIds: selectionBatchTransferIds(batch),
  };
}

function selectionBatchExpiry(batch: YasSelectionUploadBatch): bigint {
  const expiresServerNs =
    batch.transfers[0]?.descriptor.uploadStage?.expiresServerNs;
  if (expiresServerNs === undefined)
    throw new YasProtocolError("Selection stage has no upload expiry");
  for (const transfer of batch.transfers)
    if (
      transfer.descriptor.uploadStage?.stagingHandle !== batch.stagingHandle ||
      transfer.descriptor.uploadStage.expiresServerNs !== expiresServerNs
    )
      throw new YasProtocolError(
        "Selection item descriptors disagree on upload-stage identity",
      );
  return expiresServerNs;
}

function validateSlot(slot: number): void {
  if (
    slot !== YAS_SELECTION_SLOT_CLIPBOARD &&
    slot !== YAS_SELECTION_SLOT_PRIMARY
  )
    throw new YasProtocolError("unknown Selection slot");
}

function validateMime(mime: string): void {
  const length = utf8Length(mime);
  if (
    length === 0 ||
    length > YAS_SELECTION_MAX_MIME_BYTES ||
    mime.includes("\0")
  )
    throw new YasProtocolError("invalid Selection MIME");
}

function validateOrderedMimes(
  mimes: readonly string[],
  allowEmpty: boolean,
): void {
  if (
    mimes.length > YAS_SELECTION_MAX_ITEMS ||
    (!allowEmpty && mimes.length === 0)
  )
    throw new YasProtocolError("invalid Selection MIME count");
  let previous: string | undefined;
  for (const mime of mimes) {
    validateMime(mime);
    if (previous !== undefined && previous >= mime)
      throw new YasProtocolError(
        "Selection MIME types are not strictly ordered",
      );
    previous = mime;
  }
}

function decodeMimes(cursor: YasCursor, allowEmpty: boolean): string[] {
  const count = cursor.u16("Selection MIME count");
  if (
    count > YAS_SELECTION_MAX_ITEMS ||
    (!allowEmpty && count === 0) ||
    count > Math.floor(cursor.remaining / 2)
  )
    throw new YasProtocolError("invalid Selection MIME count");
  const mimes: string[] = [];
  for (let index = 0; index < count; index++)
    mimes.push(cursor.utf8U16("Selection MIME"));
  validateOrderedMimes(mimes, allowEmpty);
  return mimes;
}

function validateDragItems(items: readonly YasSelectionDragItem[]): void {
  if (items.length === 0 || items.length > YAS_SELECTION_MAX_ITEMS)
    throw new YasProtocolError("invalid Selection drag item count");
  for (const item of items) {
    validateDragName(item.name);
    validateOrderedMimes(item.mimeTypes, false);
  }
}

function encodeDragItems(
  writer: YasWriter,
  items: readonly YasSelectionDragItem[],
): void {
  for (const item of items) {
    writer.utf8U16(item.name).u16(item.mimeTypes.length);
    for (const mime of item.mimeTypes) writer.utf8U16(mime);
  }
}

function decodeDragItems(cursor: YasCursor): YasSelectionDragItem[] {
  const count = cursor.u16("Selection drag item count");
  if (
    count === 0 ||
    count > YAS_SELECTION_MAX_ITEMS ||
    count > Math.floor(cursor.remaining / 4)
  )
    throw new YasProtocolError("invalid Selection drag item count");
  const items: YasSelectionDragItem[] = [];
  for (let index = 0; index < count; index++)
    items.push({
      name: cursor.utf8U16("Selection drag item name"),
      mimeTypes: decodeMimes(cursor, false),
    });
  validateDragItems(items);
  return items;
}

function validateDragName(name: string): void {
  const bytes = new TextEncoder().encode(name);
  if (bytes.length > YAS_SELECTION_MAX_ITEM_NAME_BYTES || bytes.includes(0))
    throw new YasProtocolError("invalid Selection drag item name");
}

function validateActions(
  actions: number,
  allowNone: boolean,
  single: boolean,
): void {
  if (
    actions & ~YAS_SELECTION_ACTION_MASK ||
    (!allowNone && actions === 0) ||
    (single && (actions & (actions - 1)) !== 0)
  )
    throw new YasProtocolError("invalid Selection drag action");
}

function validateDragIdentity(value: YasSelectionDragLeave): void {
  requireHandle(value.dragHandle, "Selection drag handle");
  requireRevision(value.revision, "Selection drag revision");
  requireHandle(value.targetSurface, "Selection target surface");
}

function validateDragPosition(value: YasSelectionDragPosition): void {
  validateDragIdentity(value);
  validateActions(value.actions, true, false);
}

function requireOperationId(value: Uint8Array, context: string): void {
  if (value.length !== 16)
    throw new YasProtocolError(`${context} operation ID is not 16 bytes`);
  if (value.every((byte) => byte === 0))
    throw new YasProtocolError(`${context} operation ID is zero`);
}

function requireHash(value: Uint8Array): void {
  if (value.length !== 32)
    throw new YasProtocolError("Selection content hash is not 32 bytes");
}

function requireHandle(value: bigint, context: string): void {
  if (value === 0n) throw new YasProtocolError(`${context} is zero`);
}

function requireRevision(value: bigint, context: string): void {
  if (value === 0n) throw new YasProtocolError(`${context} is zero`);
}

function requireZero(bytes: Uint8Array, context: string): void {
  if (bytes.some((value) => value !== 0))
    throw new YasProtocolError(`${context} reserved bytes are nonzero`);
}

function utf8Length(value: string): number {
  return new TextEncoder().encode(value).length;
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
