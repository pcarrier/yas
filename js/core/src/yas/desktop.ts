/** YAS Desktop family v1 codecs and browser client. */

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

const desktopNotificationExtensionTags: ReadonlySet<number> = new Set<number>([
  g.YAS_DESKTOP_NOTIFICATION_IMAGE_HASH_EXTENSION,
  g.YAS_DESKTOP_NOTIFICATION_APPLICATION_ICON_HASH_EXTENSION,
  g.YAS_DESKTOP_NOTIFICATION_PROGRESS_EXTENSION,
  g.YAS_DESKTOP_NOTIFICATION_REPLY_EXTENSION,
]);

export {
  YAS_DESKTOP_ASSET_CONTENT_KIND,
  YAS_DESKTOP_FETCH_ASSET,
  YAS_DESKTOP_GET_MENU,
  YAS_DESKTOP_MAX_INLINE_ASSET_BYTES,
  YAS_DESKTOP_MAX_INLINE_MENU_BYTES,
  YAS_DESKTOP_MAX_MENU_NODES,
  YAS_DESKTOP_MAX_NOTIFICATION_ACTIONS,
  YAS_DESKTOP_MENU_CHECKED,
  YAS_DESKTOP_MENU_CONTENT_KIND,
  YAS_DESKTOP_MENU_ENABLED,
  YAS_DESKTOP_MENU_FLAGS_MASK,
  YAS_DESKTOP_MENU_NODE_ITEM,
  YAS_DESKTOP_MENU_NODE_ROOT,
  YAS_DESKTOP_MENU_NODE_SEPARATOR,
  YAS_DESKTOP_MENU_NODE_SUBMENU,
  YAS_DESKTOP_MENU_RADIO,
  YAS_DESKTOP_MENU_VISIBLE,
  YAS_DESKTOP_NOTIFICATION_ACTION,
  YAS_DESKTOP_NOTIFICATION_ACTION_ACTION,
  YAS_DESKTOP_NOTIFICATION_ACTION_DEFAULT,
  YAS_DESKTOP_NOTIFICATION_ACTION_DISMISS,
  YAS_DESKTOP_NOTIFICATION_APPLICATION_ICON_HASH_EXTENSION,
  YAS_DESKTOP_NOTIFICATION_CLOSED_BY_CALLER,
  YAS_DESKTOP_NOTIFICATION_CLOSED_DISMISSED,
  YAS_DESKTOP_NOTIFICATION_CLOSED_EXPIRED,
  YAS_DESKTOP_NOTIFICATION_CLOSED_UNDEFINED,
  YAS_DESKTOP_NOTIFICATION_FLAGS_MASK,
  YAS_DESKTOP_NOTIFICATION_HAS_PROGRESS,
  YAS_DESKTOP_NOTIFICATION_HAS_REPLY,
  YAS_DESKTOP_NOTIFICATION_IMAGE_HASH_EXTENSION,
  YAS_DESKTOP_NOTIFICATION_PROGRESS_EXTENSION,
  YAS_DESKTOP_NOTIFICATION_REPLY_EXTENSION,
  YAS_DESKTOP_NOTIFICATION_RESIDENT,
  YAS_DESKTOP_NOTIFICATION_TRANSIENT,
  YAS_DESKTOP_NOTIFICATION_URGENCY_CRITICAL,
  YAS_DESKTOP_NOTIFICATION_URGENCY_LOW,
  YAS_DESKTOP_NOTIFICATION_URGENCY_NORMAL,
  YAS_DESKTOP_RECORD_NOTIFICATION,
  YAS_DESKTOP_RECORD_TRAY,
  YAS_DESKTOP_STATE,
  YAS_DESKTOP_STATE_ACK,
  YAS_DESKTOP_TRAY_ACTION,
  YAS_DESKTOP_TRAY_ACTION_ACTIVATE,
  YAS_DESKTOP_TRAY_ACTION_FLAGS_MASK,
  YAS_DESKTOP_TRAY_ACTION_MENU_ITEM,
  YAS_DESKTOP_TRAY_ACTION_SCROLL,
  YAS_DESKTOP_TRAY_ACTION_SCROLL_HORIZONTAL,
  YAS_DESKTOP_TRAY_ACTION_SECONDARY_ACTIVATE,
  YAS_DESKTOP_TRAY_STATUS_ACTIVE,
  YAS_DESKTOP_TRAY_STATUS_NEEDS_ATTENTION,
  YAS_DESKTOP_TRAY_STATUS_PASSIVE,
  YAS_DESKTOP_UNWATCH,
  YAS_DESKTOP_VERSION,
  YAS_DESKTOP_WATCH,
  YAS_DESKTOP_WATCH_DATASET_EXTENSION,
  YAS_DESKTOP_WATCH_NOTIFICATIONS,
  YAS_DESKTOP_WATCH_TRAY,
  YAS_FAMILY_DESKTOP,
} from "./generated";

export interface YasDesktopMenuNode {
  nodeHandle: bigint;
  parentHandle: bigint;
  kind: number;
  flags: number;
  position: number;
  actionHandle: bigint;
  label: string;
  shortcut: string;
  iconHash: Uint8Array;
  extensions: readonly YasExtension[];
}

export interface YasDesktopMenuTree {
  trayHandle: bigint;
  trayRevision: bigint;
  menuRevision: bigint;
  nodes: readonly YasDesktopMenuNode[];
  extensions: readonly YasExtension[];
}

export interface YasDesktopTrayRecord {
  kind: "tray";
  trayHandle: bigint;
  revision: bigint;
  menuRevision: bigint;
  status: number;
  title: string;
  iconHash: Uint8Array;
  extensions: readonly YasExtension[];
}

export interface YasDesktopNotificationButton {
  actionHandle: bigint;
  label: string;
}

export interface YasDesktopNotificationRecord {
  kind: "notification";
  notificationHandle: bigint;
  revision: bigint;
  flags: number;
  urgency: number;
  expiresServerNs: bigint;
  application: string;
  summary: string;
  body: string;
  actions: readonly YasDesktopNotificationButton[];
  extensions: readonly YasExtension[];
  contentImageHash: Uint8Array | null;
  applicationIconHash: Uint8Array | null;
  progress: YasDesktopNotificationProgress | null;
  replyPlaceholder: string | null;
}

export interface YasDesktopNotificationProgress {
  value: number;
  maximum: number;
}

export type YasDesktopNotificationFieldPatch<T> =
  | { kind: "clear" }
  | { kind: "set"; value: T };

export interface YasDesktopNotificationPatch {
  notificationHandle: bigint;
  revision: bigint;
  contentImageHash?: YasDesktopNotificationFieldPatch<Uint8Array>;
  applicationIconHash?: YasDesktopNotificationFieldPatch<Uint8Array>;
  progress?: YasDesktopNotificationFieldPatch<YasDesktopNotificationProgress>;
  replyPlaceholder?: YasDesktopNotificationFieldPatch<string>;
  /** Unknown optional extensions preserved by a decoder. */
  extensions?: readonly YasExtension[];
}

export interface YasDesktopNotificationRemoval {
  notificationHandle: bigint;
  revision: bigint;
  closeReason: number;
}

export interface YasDesktopSnapshot {
  revision: bigint;
  trays: readonly YasDesktopTrayRecord[];
  notifications: readonly YasDesktopNotificationRecord[];
}

export interface YasDesktopContent {
  byteLength: bigint;
  contentHash: Uint8Array;
  bytes(): Promise<Uint8Array>;
}

export interface YasDesktopTrayAction {
  trayHandle: bigint;
  trayRevision: bigint;
  menuRevision: bigint;
  operationId: Uint8Array;
  actionKind: number;
  flags: number;
  value: number;
  itemHandle: bigint;
  extensions?: readonly YasExtension[];
}

export interface YasDesktopNotificationAction {
  notificationHandle: bigint;
  revision: bigint;
  actionKind: number;
  actionHandle: bigint;
  operationId: Uint8Array;
  reply: string;
  extensions?: readonly YasExtension[];
}

export interface YasDesktopMenuContent extends YasDesktopContent {
  menu(): Promise<YasDesktopMenuTree>;
}

export function desktopWatchDatasetExtension(datasets: number): YasExtension {
  validateDatasets(datasets);
  return {
    tag: g.YAS_DESKTOP_WATCH_DATASET_EXTENSION,
    required: false,
    value: Uint8Array.of(datasets),
  };
}

export function encodeDesktopFetchAsset(
  contentHash: Uint8Array,
  initialReceiveCredit: bigint,
  extensions: readonly YasExtension[] = [],
): Uint8Array {
  requireHash(contentHash, "Desktop asset hash");
  return new YasWriter()
    .bytes(contentHash)
    .u64(initialReceiveCredit)
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function encodeDesktopTrayAction(
  value: YasDesktopTrayAction,
): Uint8Array {
  validateTrayAction(value);
  return new YasWriter()
    .u64(value.trayHandle)
    .u64(value.trayRevision)
    .u64(value.menuRevision)
    .bytes(value.operationId)
    .u8(value.actionKind)
    .u8(value.flags)
    .u16(0)
    .i32(value.value)
    .u64(value.itemHandle)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeDesktopTrayAction(
  bytes: Uint8Array,
): YasDesktopTrayAction {
  const cursor = new YasCursor(bytes);
  const value = {
    trayHandle: cursor.u64("Desktop tray handle"),
    trayRevision: cursor.u64("Desktop tray revision"),
    menuRevision: cursor.u64("Desktop menu revision"),
    operationId: new Uint8Array(
      cursor.take(16, "Desktop tray action operation ID"),
    ),
    actionKind: cursor.u8("Desktop tray action kind"),
    flags: cursor.u8("Desktop tray action flags"),
    value: 0,
    itemHandle: 0n,
    extensions: [] as readonly YasExtension[],
  };
  if (cursor.u16("Desktop tray action reserved") !== 0)
    throw new YasProtocolError("Desktop tray action reserved is nonzero");
  value.value = cursor.i32("Desktop tray action value");
  value.itemHandle = cursor.u64("Desktop tray action item handle");
  value.extensions = decodeExtensions(
    cursor,
    new Set(),
    "Desktop tray action extensions",
  );
  cursor.end("Desktop TRAY_ACTION");
  validateTrayAction(value);
  return value;
}

export function encodeDesktopNotificationAction(
  value: YasDesktopNotificationAction,
): Uint8Array {
  validateNotificationAction(value);
  return new YasWriter()
    .u64(value.notificationHandle)
    .u64(value.revision)
    .u8(value.actionKind)
    .bytes(new Uint8Array(3))
    .u64(value.actionHandle)
    .bytes(value.operationId)
    .utf8U32(value.reply)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeDesktopNotificationAction(
  bytes: Uint8Array,
): YasDesktopNotificationAction {
  const cursor = new YasCursor(bytes);
  const notificationHandle = cursor.u64("Desktop notification handle");
  const revision = cursor.u64("Desktop notification revision");
  const actionKind = cursor.u8("Desktop notification action kind");
  requireZero(
    cursor.take(3, "Desktop notification action reserved"),
    "Desktop notification action",
  );
  const value = {
    notificationHandle,
    revision,
    actionKind,
    actionHandle: cursor.u64("Desktop notification action handle"),
    operationId: new Uint8Array(
      cursor.take(16, "Desktop notification operation ID"),
    ),
    reply: cursor.utf8U32("Desktop notification reply"),
    extensions: decodeExtensions(
      cursor,
      new Set(),
      "Desktop notification action extensions",
    ),
  };
  cursor.end("Desktop NOTIFICATION_ACTION");
  validateNotificationAction(value);
  return value;
}

export function decodeDesktopMenuNode(bytes: Uint8Array): YasDesktopMenuNode {
  const cursor = new YasCursor(bytes);
  const value: YasDesktopMenuNode = {
    nodeHandle: cursor.u64("Desktop menu node handle"),
    parentHandle: cursor.u64("Desktop menu parent handle"),
    kind: cursor.u8("Desktop menu node kind"),
    flags: cursor.u8("Desktop menu node flags"),
    position: 0,
    actionHandle: 0n,
    label: "",
    shortcut: "",
    iconHash: new Uint8Array(0),
    extensions: [],
  };
  if (cursor.u16("Desktop menu node reserved") !== 0)
    throw new YasProtocolError("Desktop menu node reserved field is nonzero");
  value.position = cursor.u32("Desktop menu position");
  value.actionHandle = cursor.u64("Desktop menu action handle");
  value.label = cursor.utf8U16("Desktop menu label");
  value.shortcut = cursor.utf8U16("Desktop menu shortcut");
  value.iconHash = new Uint8Array(cursor.take(32, "Desktop menu icon hash"));
  value.extensions = decodeExtensions(
    cursor,
    new Set(),
    "Desktop menu node extensions",
  );
  cursor.end("Desktop menu node");
  validateMenuNode(value);
  return value;
}

export function encodeDesktopMenuNode(value: YasDesktopMenuNode): Uint8Array {
  validateMenuNode(value);
  return new YasWriter()
    .u64(value.nodeHandle)
    .u64(value.parentHandle)
    .u8(value.kind)
    .u8(value.flags)
    .u16(0)
    .u32(value.position)
    .u64(value.actionHandle)
    .utf8U16(value.label)
    .utf8U16(value.shortcut)
    .bytes(value.iconHash)
    .bytes(encodeExtensions(value.extensions))
    .finish();
}

export function decodeDesktopMenuTree(bytes: Uint8Array): YasDesktopMenuTree {
  const cursor = new YasCursor(bytes);
  const trayHandle = cursor.u64("Desktop menu tray handle");
  const trayRevision = cursor.u64("Desktop tray revision");
  const menuRevision = cursor.u64("Desktop menu revision");
  const count = cursor.u32("Desktop menu node count");
  if (
    count === 0 ||
    count > g.YAS_DESKTOP_MAX_MENU_NODES ||
    count > Math.floor(cursor.remaining / 4)
  )
    throw new YasProtocolError("invalid Desktop menu node count");
  const nodes: YasDesktopMenuNode[] = [];
  for (let index = 0; index < count; index++)
    nodes.push(decodeDesktopMenuNode(cursor.bytesU32("Desktop menu node")));
  const extensions = decodeExtensions(
    cursor,
    new Set(),
    "Desktop menu extensions",
  );
  cursor.end("Desktop menu tree");
  const value = { trayHandle, trayRevision, menuRevision, nodes, extensions };
  validateMenuTree(value);
  return value;
}

export function encodeDesktopMenuTree(value: YasDesktopMenuTree): Uint8Array {
  validateMenuTree(value);
  const writer = new YasWriter()
    .u64(value.trayHandle)
    .u64(value.trayRevision)
    .u64(value.menuRevision)
    .u32(value.nodes.length);
  for (const node of value.nodes) writer.bytesU32(encodeDesktopMenuNode(node));
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeDesktopTrayRecord(
  bytes: Uint8Array,
): YasDesktopTrayRecord {
  const cursor = new YasCursor(bytes);
  const value: YasDesktopTrayRecord = {
    kind: "tray",
    trayHandle: cursor.u64("Desktop tray handle"),
    revision: cursor.u64("Desktop tray revision"),
    menuRevision: cursor.u64("Desktop menu revision"),
    status: cursor.u8("Desktop tray status"),
    title: "",
    iconHash: new Uint8Array(0),
    extensions: [],
  };
  requireZero(cursor.take(3, "Desktop tray reserved"), "Desktop tray");
  value.title = cursor.utf8U16("Desktop tray title");
  value.iconHash = new Uint8Array(cursor.take(32, "Desktop tray icon hash"));
  value.extensions = decodeExtensions(
    cursor,
    new Set(),
    "Desktop tray extensions",
  );
  cursor.end("Desktop tray record");
  validateTray(value);
  return value;
}

export function decodeDesktopNotificationRecord(
  bytes: Uint8Array,
): YasDesktopNotificationRecord {
  const cursor = new YasCursor(bytes);
  const notificationHandle = cursor.u64("Desktop notification handle");
  const revision = cursor.u64("Desktop notification revision");
  const flags = cursor.u16("Desktop notification flags");
  const urgency = cursor.u8("Desktop notification urgency");
  if (cursor.u8("Desktop notification reserved") !== 0)
    throw new YasProtocolError("Desktop notification reserved is nonzero");
  const expiresServerNs = cursor.u64("Desktop notification expiry");
  const application = cursor.utf8U16("Desktop notification application");
  const summary = cursor.utf8U16("Desktop notification summary");
  const body = cursor.utf8U32("Desktop notification body");
  const count = cursor.u16("Desktop notification action count");
  if (
    count > g.YAS_DESKTOP_MAX_NOTIFICATION_ACTIONS ||
    count > Math.floor(cursor.remaining / 10)
  )
    throw new YasProtocolError("invalid Desktop notification action count");
  const actions: YasDesktopNotificationButton[] = [];
  for (let index = 0; index < count; index++)
    actions.push({
      actionHandle: cursor.u64("Desktop notification action handle"),
      label: cursor.utf8U16("Desktop notification action label"),
    });
  const extensions = decodeExtensions(
    cursor,
    desktopNotificationExtensionTags,
    "Desktop notification extensions",
  );
  cursor.end("Desktop notification record");
  const raw = {
    kind: "notification" as const,
    notificationHandle,
    revision,
    flags,
    urgency,
    expiresServerNs,
    application,
    summary,
    body,
    actions,
    extensions,
  };
  const value = {
    ...raw,
    ...decodeDesktopNotificationMetadata(extensions, false),
  };
  validateNotification(value);
  return value;
}

export function encodeDesktopNotificationRecord(
  value: YasDesktopNotificationRecord,
): Uint8Array {
  validateNotification(value);
  const writer = new YasWriter()
    .u64(value.notificationHandle)
    .u64(value.revision)
    .u16(value.flags)
    .u8(value.urgency)
    .u8(0)
    .u64(value.expiresServerNs)
    .utf8U16(value.application)
    .utf8U16(value.summary)
    .utf8U32(value.body)
    .u16(value.actions.length);
  for (const action of value.actions)
    writer.u64(action.actionHandle).utf8U16(action.label);
  return writer.bytes(encodeExtensions(value.extensions)).finish();
}

export function decodeDesktopNotificationPatch(
  bytes: Uint8Array,
): YasDesktopNotificationPatch {
  const cursor = new YasCursor(bytes);
  if (
    cursor.u16("Desktop notification patch entity") !==
    g.YAS_DESKTOP_RECORD_NOTIFICATION
  )
    throw new YasProtocolError(
      "Desktop notification patch has the wrong entity",
    );
  if (cursor.u16("Desktop notification patch reserved") !== 0)
    throw new YasProtocolError(
      "Desktop notification patch reserved is nonzero",
    );
  const notificationHandle = cursor.u64("Desktop notification patch handle");
  const revision = cursor.u64("Desktop notification patch revision");
  requireHandle(notificationHandle, "Desktop notification patch handle");
  requireRevision(revision, "Desktop notification patch revision");
  const decoded = decodeExtensions(
    cursor,
    desktopNotificationExtensionTags,
    "Desktop notification patch extensions",
  );
  cursor.end("Desktop notification patch");
  const value: YasDesktopNotificationPatch = {
    notificationHandle,
    revision,
    extensions: [],
  };
  const unknown: YasExtension[] = [];
  for (const extension of decoded) {
    if (extension.tag === g.YAS_DESKTOP_NOTIFICATION_IMAGE_HASH_EXTENSION)
      value.contentImageHash = decodeDesktopHashPatch(
        extension.value,
        "Desktop notification content image hash patch",
      );
    else if (
      extension.tag ===
      g.YAS_DESKTOP_NOTIFICATION_APPLICATION_ICON_HASH_EXTENSION
    )
      value.applicationIconHash = decodeDesktopHashPatch(
        extension.value,
        "Desktop notification application icon hash patch",
      );
    else if (extension.tag === g.YAS_DESKTOP_NOTIFICATION_PROGRESS_EXTENSION)
      value.progress =
        extension.value.length === 0
          ? { kind: "clear" }
          : {
              kind: "set",
              value: decodeDesktopNotificationProgress(extension.value),
            };
    else if (extension.tag === g.YAS_DESKTOP_NOTIFICATION_REPLY_EXTENSION)
      value.replyPlaceholder =
        extension.value.length === 0
          ? { kind: "clear" }
          : {
              kind: "set",
              value: decodeDesktopNotificationReply(extension.value),
            };
    else unknown.push(extension);
  }
  value.extensions = unknown;
  return value;
}

export function encodeDesktopNotificationPatch(
  value: YasDesktopNotificationPatch,
): Uint8Array {
  requireHandle(value.notificationHandle, "Desktop notification patch handle");
  requireRevision(value.revision, "Desktop notification patch revision");
  const extensions = [...(value.extensions ?? [])];
  const add = (
    tag: number,
    field: YasDesktopNotificationFieldPatch<unknown> | undefined,
    encode: (value: never) => Uint8Array,
  ) => {
    if (!field) return;
    extensions.push({
      tag,
      value:
        field.kind === "clear"
          ? new Uint8Array()
          : encode(field.value as never),
    });
  };
  add(
    g.YAS_DESKTOP_NOTIFICATION_IMAGE_HASH_EXTENSION,
    value.contentImageHash,
    (hash: Uint8Array) =>
      encodeDesktopHash(hash, "Desktop notification content image hash patch"),
  );
  add(
    g.YAS_DESKTOP_NOTIFICATION_APPLICATION_ICON_HASH_EXTENSION,
    value.applicationIconHash,
    (hash: Uint8Array) =>
      encodeDesktopHash(
        hash,
        "Desktop notification application icon hash patch",
      ),
  );
  add(
    g.YAS_DESKTOP_NOTIFICATION_PROGRESS_EXTENSION,
    value.progress,
    encodeDesktopNotificationProgress,
  );
  add(
    g.YAS_DESKTOP_NOTIFICATION_REPLY_EXTENSION,
    value.replyPlaceholder,
    (placeholder: string) => new YasWriter().utf8U16(placeholder).finish(),
  );
  extensions.sort((left, right) => left.tag - right.tag);
  validateDesktopNotificationPatchExtensions(extensions);
  return new YasWriter()
    .u16(g.YAS_DESKTOP_RECORD_NOTIFICATION)
    .u16(0)
    .u64(value.notificationHandle)
    .u64(value.revision)
    .bytes(encodeExtensions(extensions))
    .finish();
}

export function decodeDesktopNotificationRemoval(
  bytes: Uint8Array,
): YasDesktopNotificationRemoval {
  const cursor = new YasCursor(bytes);
  if (
    cursor.u16("Desktop notification removal entity") !==
    g.YAS_DESKTOP_RECORD_NOTIFICATION
  )
    throw new YasProtocolError(
      "Desktop notification removal has the wrong entity",
    );
  if (cursor.u16("Desktop notification removal reserved") !== 0)
    throw new YasProtocolError(
      "Desktop notification removal reserved is nonzero",
    );
  const value = {
    notificationHandle: cursor.u64("Desktop notification removal handle"),
    revision: cursor.u64("Desktop notification removal revision"),
    closeReason: cursor.u8("Desktop notification close reason"),
  };
  requireZero(
    cursor.take(3, "Desktop notification removal reserved tail"),
    "Desktop notification removal",
  );
  cursor.end("Desktop notification removal");
  validateDesktopNotificationRemoval(value);
  return value;
}

export function encodeDesktopNotificationRemoval(
  value: YasDesktopNotificationRemoval,
): Uint8Array {
  validateDesktopNotificationRemoval(value);
  return new YasWriter()
    .u16(g.YAS_DESKTOP_RECORD_NOTIFICATION)
    .u16(0)
    .u64(value.notificationHandle)
    .u64(value.revision)
    .u8(value.closeReason)
    .bytes(new Uint8Array(3))
    .finish();
}

export class YasDesktopCatalog {
  private trays = new Map<bigint, YasDesktopTrayRecord>();
  private notifications = new Map<bigint, YasDesktopNotificationRecord>();
  private staging: {
    trays: Map<bigint, YasDesktopTrayRecord>;
    notifications: Map<bigint, YasDesktopNotificationRecord>;
  } | null = null;
  private subscription: YasStateSubscription | null = null;
  private retention: YasStateCatalogueRetention<string>;
  private stagingRetention: YasStateCatalogueRetention<string> | null = null;
  private revision = 0n;
  private listeners = new Set<(snapshot: YasDesktopSnapshot) => void>();
  private pendingFirstSnapshots = new Set<(error: unknown) => void>();
  private notificationRemovalListeners = new Set<
    (removal: YasDesktopNotificationRemoval) => void
  >();
  private readonly removeInvalidation: () => void;
  private pendingWatch: Promise<void> | null = null;
  private pendingWatchCancel: ((error: unknown) => void) | null = null;
  private watchEpoch = 0;
  private disposed = false;

  constructor(private readonly connection: YasConnection) {
    this.retention = YasStateCatalogueRetention.forConnection(connection);
    this.removeInvalidation = connection.onInvalidation(({ family }) => {
      if (family === undefined || family === g.YAS_FAMILY_DESKTOP) {
        this.cancelPendingWatch(
          new YasProtocolError("Desktop catalogue was invalidated"),
        );
        this.resetLocal();
      }
    });
  }

  get snapshot(): YasDesktopSnapshot {
    return {
      revision: this.revision,
      trays: [...this.trays.values()],
      notifications: [...this.notifications.values()],
    };
  }

  subscribe(listener: (snapshot: YasDesktopSnapshot) => void): () => void {
    if (this.disposed) throw new Error("Desktop catalogue is disposed");
    this.listeners.add(listener);
    try {
      listener(this.snapshot);
    } catch {
      this.listeners.delete(listener);
    }
    return () => this.listeners.delete(listener);
  }

  onNotificationRemoved(
    listener: (removal: YasDesktopNotificationRemoval) => void,
  ): () => void {
    this.notificationRemovalListeners.add(listener);
    return () => this.notificationRemovalListeners.delete(listener);
  }

  async firstSnapshot(
    options: YasWatchOptions = {},
    datasets = g.YAS_DESKTOP_WATCH_TRAY | g.YAS_DESKTOP_WATCH_NOTIFICATIONS,
  ): Promise<YasDesktopSnapshot> {
    if (this.disposed) throw new Error("Desktop catalogue is disposed");
    if (this.revision !== 0n && this.subscription?.active) return this.snapshot;
    let remove: (() => void) | undefined;
    let rejectPending!: (error: unknown) => void;
    const result = new Promise<YasDesktopSnapshot>((resolve, reject) => {
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
        this.watch(options, datasets).then(() => result),
      ]);
    } finally {
      remove?.();
      this.pendingFirstSnapshots.delete(rejectPending);
    }
  }

  watch(
    options: YasWatchOptions = {},
    datasets = g.YAS_DESKTOP_WATCH_TRAY | g.YAS_DESKTOP_WATCH_NOTIFICATIONS,
  ): Promise<void> {
    if (this.disposed)
      return Promise.reject(new Error("Desktop catalogue is disposed"));
    if (this.subscription?.active) return Promise.resolve();
    if (this.pendingWatch) return this.pendingWatch;
    validateDatasets(datasets);
    const existing = options.extensions ?? [];
    if (
      existing.some(
        (extension) => extension.tag === g.YAS_DESKTOP_WATCH_DATASET_EXTENSION,
      )
    )
      throw new YasProtocolError(
        "Desktop WATCH dataset extension was duplicated",
      );
    const extensions = [
      ...existing,
      desktopWatchDatasetExtension(datasets),
    ].sort((left, right) => left.tag - right.tag);
    this.resetLocal();
    const epoch = this.watchEpoch;
    const watched = YasStateSubscription.watch(
      this.connection,
      g.YAS_FAMILY_DESKTOP,
      g.YAS_DESKTOP_WATCH,
      g.YAS_DESKTOP_UNWATCH,
      g.YAS_DESKTOP_STATE,
      g.YAS_DESKTOP_STATE_ACK,
      { ...options, extensions },
      (batch) => {
        if (!this.disposed && epoch === this.watchEpoch) this.apply(batch);
      },
    ).then(async (subscription) => {
      if (this.disposed || epoch !== this.watchEpoch) {
        await subscription.unwatch().catch(() => undefined);
        throw new YasProtocolError("Desktop catalogue watch was cancelled");
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
      new YasProtocolError("Desktop catalogue watch was cancelled"),
    );
    const subscription = this.subscription;
    this.subscription = null;
    if (!this.disposed) this.clearState();
    await subscription?.unwatch();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    const disposalError = new Error("Desktop catalogue is disposed");
    this.cancelPendingWatch(disposalError);
    this.removeInvalidation();
    for (const reject of [...this.pendingFirstSnapshots]) reject(disposalError);
    this.pendingFirstSnapshots.clear();
    this.listeners.clear();
    this.notificationRemovalListeners.clear();
    const subscription = this.subscription;
    this.subscription = null;
    this.retention.dispose();
    this.stagingRetention?.dispose();
    this.trays.clear();
    this.notifications.clear();
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
      this.staging = { trays: new Map(), notifications: new Map() };
      this.stagingRetention = YasStateCatalogueRetention.forConnection(
        this.connection,
      );
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_RECORDS) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("Desktop snapshot records without begin");
      try {
        this.applyRecords(
          this.staging.trays,
          this.staging.notifications,
          this.stagingRetention,
          batch.records,
        );
        this.validateCatalog(this.staging.trays, this.staging.notifications);
      } catch (error) {
        this.discardStaging();
        throw error;
      }
      return;
    }
    if (batch.phase === YAS_STATE_SNAPSHOT_END) {
      if (!this.staging || !this.stagingRetention)
        throw new YasProtocolError("Desktop snapshot end without begin");
      try {
        this.applyRecords(
          this.staging.trays,
          this.staging.notifications,
          this.stagingRetention,
          batch.records,
        );
        this.validateCatalog(this.staging.trays, this.staging.notifications);
      } catch (error) {
        this.discardStaging();
        throw error;
      }
      const previousRetention = this.retention;
      this.trays = this.staging.trays;
      this.notifications = this.staging.notifications;
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
      let trays: Map<bigint, YasDesktopTrayRecord>;
      let notifications: Map<bigint, YasDesktopNotificationRecord>;
      let removals: YasDesktopNotificationRemoval[];
      try {
        trays = new Map(this.trays);
        notifications = new Map(this.notifications);
        removals = this.applyRecords(
          trays,
          notifications,
          retention,
          batch.records,
        );
        this.validateCatalog(trays, notifications);
      } catch (error) {
        retention.dispose();
        throw error;
      }
      const previousRetention = this.retention;
      this.trays = trays;
      this.notifications = notifications;
      this.retention = retention;
      previousRetention.dispose();
      this.revision = batch.toRevision;
      this.emit();
      for (const removal of removals)
        for (const listener of this.notificationRemovalListeners) {
          try {
            listener(removal);
          } catch {
            // One observer cannot block sibling dataset delivery.
          }
        }
    }
  }

  private applyRecords(
    trays: Map<bigint, YasDesktopTrayRecord>,
    notifications: Map<bigint, YasDesktopNotificationRecord>,
    retention: YasStateCatalogueRetention<string>,
    records: readonly YasTypedRecord[],
  ): YasDesktopNotificationRemoval[] {
    const removals: YasDesktopNotificationRemoval[] = [];
    for (const action of records) {
      const cursor = new YasCursor(action.body);
      const entity = cursor.u16("Desktop state entity");
      if (cursor.u16("Desktop state reserved") !== 0)
        throw new YasProtocolError("Desktop state reserved field is nonzero");
      const body = new Uint8Array(cursor.take(cursor.remaining));
      if (entity === g.YAS_DESKTOP_RECORD_TRAY) {
        if (action.kind === YAS_STATE_ADD && trays.size >= this.trayLimit())
          throw new YasProtocolError(
            "Desktop catalogue exceeds its negotiated entity limits",
          );
        applyDesktopEntity(
          trays,
          action.kind,
          body,
          decodeDesktopTrayRecord,
          estimateStateRetainedBytes,
          (record) => record.trayHandle,
          new Set(),
          retention,
          "tray",
        );
      } else if (entity === g.YAS_DESKTOP_RECORD_NOTIFICATION) {
        if (
          action.kind === YAS_STATE_ADD &&
          notifications.size >= this.notificationLimit()
        )
          throw new YasProtocolError(
            "Desktop catalogue exceeds its negotiated entity limits",
          );
        const removal = applyDesktopNotificationEntity(
          notifications,
          action.kind,
          body,
          retention,
        );
        if (removal) removals.push(removal);
      } else throw new YasProtocolError("unknown Desktop state entity");
    }
    return removals;
  }

  private validateCatalog(
    trays: ReadonlyMap<bigint, YasDesktopTrayRecord>,
    notifications: ReadonlyMap<bigint, YasDesktopNotificationRecord>,
  ): void {
    if (
      trays.size > this.trayLimit() ||
      notifications.size > this.notificationLimit()
    )
      throw new YasProtocolError(
        "Desktop catalogue exceeds its negotiated entity limits",
      );
  }

  private trayLimit(): number {
    return negotiatedStateLimitU32(
      this.connection,
      g.YAS_FAMILY_DESKTOP,
      g.YAS_DESKTOP_VERSION,
      g.YAS_DESKTOP_LIMIT_MAX_TRAY_ITEMS,
      g.YAS_DESKTOP_MAX_TRAY_ITEMS,
    );
  }

  private notificationLimit(): number {
    return negotiatedStateLimitU32(
      this.connection,
      g.YAS_FAMILY_DESKTOP,
      g.YAS_DESKTOP_VERSION,
      g.YAS_DESKTOP_LIMIT_MAX_NOTIFICATIONS,
      g.YAS_DESKTOP_MAX_NOTIFICATIONS,
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
    this.trays = new Map();
    this.notifications = new Map();
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

export class YasDesktopClient {
  readonly catalog: YasDesktopCatalog;
  private readonly transfers;

  constructor(readonly connection: YasConnection) {
    this.catalog = new YasDesktopCatalog(connection);
    this.transfers = transfersFor(connection);
  }

  list(
    options: YasWatchOptions = {},
    datasets = g.YAS_DESKTOP_WATCH_TRAY | g.YAS_DESKTOP_WATCH_NOTIFICATIONS,
  ): Promise<YasDesktopSnapshot> {
    return this.catalog.firstSnapshot(options, datasets);
  }

  async getMenu(
    trayHandle: bigint,
    trayRevision: bigint,
    menuRevision: bigint,
    initialReceiveCredit = 1024n * 1024n,
    extensions: readonly YasExtension[] = [],
  ): Promise<YasDesktopMenuContent> {
    requireHandle(trayHandle, "Desktop tray handle");
    requireRevision(trayRevision, "Desktop tray revision");
    requireRevision(menuRevision, "Desktop menu revision");
    const lease = this.transfers.reserveReceiveCredit(
      initialReceiveCredit,
      1024n,
    );
    let accepted = false;
    try {
      return await this.connection.requestDecoded(
        g.YAS_FAMILY_DESKTOP,
        g.YAS_DESKTOP_GET_MENU,
        new YasWriter()
          .u64(trayHandle)
          .u64(trayRevision)
          .u64(menuRevision)
          .u64(lease.bytes)
          .bytes(encodeExtensions(extensions))
          .finish(),
        (body) => {
          const delivery = decodeInlineOrTransfer(body);
          if (delivery.delivery === "inline") {
            if (delivery.bytes.length > g.YAS_DESKTOP_MAX_INLINE_MENU_BYTES)
              throw new YasProtocolError(
                "Desktop inline menu exceeds its limit",
              );
            const decoded = decodeDesktopMenuTree(delivery.bytes);
            lease.release();
            accepted = true;
            const bytes = new Uint8Array(delivery.bytes);
            return desktopMenuContent(
              delivery.byteLength,
              delivery.contentHash,
              async () => bytes,
              decoded,
            );
          }
          validateDesktopTransfer(
            delivery.descriptor,
            g.YAS_DESKTOP_MENU_CONTENT_KIND,
          );
          const transfer = this.transfers.acceptServerDescriptor(
            delivery.descriptor,
            lease,
          );
          accepted = true;
          let collected: Promise<Uint8Array> | undefined;
          return desktopMenuContent(
            delivery.byteLength,
            delivery.contentHash,
            () => (collected ??= transfer.collect(delivery.byteLength)),
          );
        },
      );
    } catch (error) {
      if (!accepted) lease.release();
      throw error;
    }
  }

  async trayAction(value: YasDesktopTrayAction): Promise<void> {
    await this.connection.request(
      g.YAS_FAMILY_DESKTOP,
      g.YAS_DESKTOP_TRAY_ACTION,
      encodeDesktopTrayAction(value),
    );
  }

  async notificationAction(value: YasDesktopNotificationAction): Promise<void> {
    await this.connection.request(
      g.YAS_FAMILY_DESKTOP,
      g.YAS_DESKTOP_NOTIFICATION_ACTION,
      encodeDesktopNotificationAction(value),
    );
  }

  fetchAsset(
    contentHash: Uint8Array,
    initialReceiveCredit = 1024n * 1024n,
    extensions: readonly YasExtension[] = [],
  ): Promise<YasDesktopContent> {
    return this.fetchContent(
      contentHash,
      initialReceiveCredit,
      extensions,
      g.YAS_DESKTOP_ASSET_CONTENT_KIND,
      g.YAS_DESKTOP_MAX_INLINE_ASSET_BYTES,
    );
  }

  private async fetchContent(
    contentHash: Uint8Array,
    initialReceiveCredit: bigint,
    extensions: readonly YasExtension[],
    contentKind: number,
    inlineLimit: number,
  ): Promise<YasDesktopContent> {
    const lease = this.transfers.reserveReceiveCredit(
      initialReceiveCredit,
      1024n,
    );
    let accepted = false;
    try {
      return await this.connection.requestDecoded(
        g.YAS_FAMILY_DESKTOP,
        g.YAS_DESKTOP_FETCH_ASSET,
        encodeDesktopFetchAsset(contentHash, lease.bytes, extensions),
        (body) => {
          const delivery = decodeInlineOrTransfer(body);
          if (delivery.delivery === "inline") {
            if (delivery.bytes.length > inlineLimit)
              throw new YasProtocolError(
                "Desktop inline content exceeds its limit",
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
          validateDesktopTransfer(delivery.descriptor, contentKind);
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

  dispose(): void {
    this.catalog.dispose();
  }
}

function applyDesktopNotificationEntity(
  target: Map<bigint, YasDesktopNotificationRecord>,
  action: number,
  body: Uint8Array,
  retention: YasStateCatalogueRetention<string>,
): YasDesktopNotificationRemoval | null {
  if (action === YAS_STATE_ADD || action === YAS_STATE_REPLACE) {
    const record = detachStateRetainedValue(
      decodeDesktopNotificationRecord(body),
    );
    const exists = target.has(record.notificationHandle);
    if ((action === YAS_STATE_ADD) === exists)
      throw new YasProtocolError(
        "Desktop notification ADD/REPLACE precondition failed",
      );
    retention.upsert(
      `notification:${record.notificationHandle}`,
      estimateStateRetainedBytes(record),
    );
    target.set(record.notificationHandle, record);
    return null;
  }
  const cursor = new YasCursor(body);
  const notificationHandle = cursor.u64("Desktop notification handle");
  const revision = cursor.u64("Desktop notification revision");
  requireHandle(notificationHandle, "Desktop notification handle");
  requireRevision(revision, "Desktop notification revision");
  if (action === YAS_STATE_PATCH) {
    const patch = decodeExtensions(
      cursor,
      desktopNotificationExtensionTags,
      "Desktop notification patch",
    );
    cursor.end("Desktop notification PATCH");
    validateDesktopNotificationPatchExtensions(patch);
    const previous = target.get(notificationHandle);
    if (!previous)
      throw new YasProtocolError(
        "Desktop notification PATCH names an unknown entity",
      );
    const extensions = mergeDesktopNotificationExtensions(
      previous.extensions,
      patch,
    );
    const next = detachStateRetainedValue(
      applyDesktopNotificationMetadata({
        ...previous,
        revision,
        extensions,
      }),
    );
    validateNotification(next);
    retention.upsert(
      `notification:${notificationHandle}`,
      estimateStateRetainedBytes(next),
    );
    target.set(notificationHandle, next);
    return null;
  }
  if (action === YAS_STATE_REMOVE) {
    const closeReason = cursor.u8("Desktop notification close reason");
    requireZero(
      cursor.take(3, "Desktop notification removal reserved"),
      "Desktop notification removal",
    );
    cursor.end("Desktop notification REMOVE");
    const removal = { notificationHandle, revision, closeReason };
    validateDesktopNotificationRemoval(removal);
    if (!target.has(notificationHandle))
      throw new YasProtocolError(
        "Desktop notification REMOVE names an unknown entity",
      );
    retention.remove(`notification:${notificationHandle}`);
    target.delete(notificationHandle);
    return removal;
  }
  throw new YasProtocolError("unknown Desktop notification state action");
}

function applyDesktopEntity<
  T extends { revision: bigint; extensions: readonly YasExtension[] },
>(
  target: Map<bigint, T>,
  action: number,
  body: Uint8Array,
  decode: (bytes: Uint8Array) => T,
  estimate: (record: T) => number,
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
      throw new YasProtocolError("Desktop ADD/REPLACE precondition failed");
    retention.upsert(`${retentionPrefix}:${key}`, estimate(record));
    target.set(key, record);
    return;
  }
  const cursor = new YasCursor(body);
  const handle = cursor.u64("Desktop entity handle");
  const revision = cursor.u64("Desktop entity revision");
  requireHandle(handle, "Desktop entity handle");
  requireRevision(revision, "Desktop entity revision");
  if (action === YAS_STATE_PATCH) {
    const extensions = decodeExtensions(
      cursor,
      extensionTags,
      "Desktop entity patch",
    );
    cursor.end("Desktop PATCH");
    const previous = target.get(handle);
    if (!previous)
      throw new YasProtocolError("Desktop PATCH names an unknown entity");
    const next = detachStateRetainedValue({
      ...previous,
      revision,
      extensions: mergeExtensions(previous.extensions, extensions),
    });
    retention.upsert(`${retentionPrefix}:${handle}`, estimate(next));
    target.set(handle, next);
  } else if (action === YAS_STATE_REMOVE) {
    cursor.end("Desktop REMOVE");
    if (!target.has(handle))
      throw new YasProtocolError("Desktop REMOVE names an unknown entity");
    retention.remove(`${retentionPrefix}:${handle}`);
    target.delete(handle);
  }
}

function desktopMenuContent(
  byteLength: bigint,
  contentHash: Uint8Array,
  load: () => Promise<Uint8Array>,
  decoded?: YasDesktopMenuTree,
): YasDesktopMenuContent {
  let bytesPromise: Promise<Uint8Array> | undefined;
  let menuPromise: Promise<YasDesktopMenuTree> | undefined = decoded
    ? Promise.resolve(decoded)
    : undefined;
  const bytes = () => (bytesPromise ??= load());
  return {
    byteLength,
    contentHash,
    bytes,
    menu: () => (menuPromise ??= bytes().then(decodeDesktopMenuTree)),
  };
}

function transferContent(
  byteLength: bigint,
  contentHash: Uint8Array,
  transfer: YasTransfer,
): YasDesktopContent {
  let collected: Promise<Uint8Array> | undefined;
  return {
    byteLength,
    contentHash,
    bytes: () => (collected ??= transfer.collect(byteLength)),
  };
}

function validateDesktopTransfer(
  descriptor: YasTransferDescriptor,
  contentKind: number,
): void {
  if (
    descriptor.mode !== YAS_TRANSFER_MODE_BYTE ||
    descriptor.direction !== YAS_TRANSFER_SENDER_TO_RECEIVER ||
    descriptor.contentFamily !== g.YAS_FAMILY_DESKTOP ||
    descriptor.contentKind !== contentKind ||
    descriptor.contentVersion !== g.YAS_DESKTOP_VERSION ||
    !descriptor.sensitiveContent
  )
    throw new YasProtocolError("invalid Desktop Transfer descriptor");
}

function validateDatasets(datasets: number): void {
  const known = g.YAS_DESKTOP_WATCH_TRAY | g.YAS_DESKTOP_WATCH_NOTIFICATIONS;
  if (!Number.isInteger(datasets) || datasets === 0 || datasets & ~known)
    throw new YasProtocolError("invalid Desktop WATCH datasets");
}

function validateMenuNode(value: YasDesktopMenuNode): void {
  requireHandle(value.nodeHandle, "Desktop menu node handle");
  requireHash(value.iconHash, "Desktop menu icon hash");
  if (
    value.kind > g.YAS_DESKTOP_MENU_NODE_SUBMENU ||
    value.flags & ~g.YAS_DESKTOP_MENU_FLAGS_MASK
  )
    throw new YasProtocolError("invalid Desktop menu node kind or flags");
  const root = value.kind === g.YAS_DESKTOP_MENU_NODE_ROOT;
  const separator = value.kind === g.YAS_DESKTOP_MENU_NODE_SEPARATOR;
  if (
    root !== (value.parentHandle === 0n) ||
    (root && value.actionHandle !== 0n) ||
    (separator && (value.label.length !== 0 || value.actionHandle !== 0n))
  )
    throw new YasProtocolError("invalid Desktop menu node shape");
}

function validateMenuTree(value: YasDesktopMenuTree): void {
  requireHandle(value.trayHandle, "Desktop menu tray handle");
  requireRevision(value.trayRevision, "Desktop tray revision");
  requireRevision(value.menuRevision, "Desktop menu revision");
  if (
    value.nodes.length === 0 ||
    value.nodes.length > g.YAS_DESKTOP_MAX_MENU_NODES
  )
    throw new YasProtocolError("invalid Desktop menu node count");
  const seen = new Set<bigint>();
  for (let index = 0; index < value.nodes.length; index++) {
    const node = value.nodes[index]!;
    validateMenuNode(node);
    if (
      seen.has(node.nodeHandle) ||
      (index === 0 && node.kind !== g.YAS_DESKTOP_MENU_NODE_ROOT) ||
      (index !== 0 && !seen.has(node.parentHandle))
    )
      throw new YasProtocolError("invalid Desktop menu preorder");
    seen.add(node.nodeHandle);
  }
}

function validateTray(value: YasDesktopTrayRecord): void {
  requireHandle(value.trayHandle, "Desktop tray handle");
  requireRevision(value.revision, "Desktop tray revision");
  requireRevision(value.menuRevision, "Desktop menu revision");
  requireHash(value.iconHash, "Desktop tray icon hash");
  if (value.status > g.YAS_DESKTOP_TRAY_STATUS_NEEDS_ATTENTION)
    throw new YasProtocolError("invalid Desktop tray status");
}

function validateNotification(value: YasDesktopNotificationRecord): void {
  requireHandle(value.notificationHandle, "Desktop notification handle");
  requireRevision(value.revision, "Desktop notification revision");
  if (
    value.flags & ~g.YAS_DESKTOP_NOTIFICATION_FLAGS_MASK ||
    value.urgency > g.YAS_DESKTOP_NOTIFICATION_URGENCY_CRITICAL ||
    value.actions.length > g.YAS_DESKTOP_MAX_NOTIFICATION_ACTIONS
  )
    throw new YasProtocolError("invalid Desktop notification flags or actions");
  const handles = new Set<bigint>();
  for (const action of value.actions) {
    requireHandle(action.actionHandle, "Desktop notification action handle");
    if (handles.has(action.actionHandle))
      throw new YasProtocolError("duplicate Desktop notification action");
    handles.add(action.actionHandle);
  }
  const metadata = decodeDesktopNotificationMetadata(value.extensions, false);
  const hasProgress = metadata.progress !== null;
  const hasReply = metadata.replyPlaceholder !== null;
  if (
    hasProgress !==
      ((value.flags & g.YAS_DESKTOP_NOTIFICATION_HAS_PROGRESS) !== 0) ||
    hasReply !== ((value.flags & g.YAS_DESKTOP_NOTIFICATION_HAS_REPLY) !== 0) ||
    !sameOptionalHash(value.contentImageHash, metadata.contentImageHash) ||
    !sameOptionalHash(
      value.applicationIconHash,
      metadata.applicationIconHash,
    ) ||
    !sameDesktopProgress(value.progress, metadata.progress) ||
    value.replyPlaceholder !== metadata.replyPlaceholder
  )
    throw new YasProtocolError("invalid Desktop notification metadata");
}

function applyDesktopNotificationMetadata(
  value: Omit<
    YasDesktopNotificationRecord,
    "contentImageHash" | "applicationIconHash" | "progress" | "replyPlaceholder"
  >,
): YasDesktopNotificationRecord {
  const metadata = decodeDesktopNotificationMetadata(value.extensions, false);
  const flags =
    (value.flags &
      ~(
        g.YAS_DESKTOP_NOTIFICATION_HAS_PROGRESS |
        g.YAS_DESKTOP_NOTIFICATION_HAS_REPLY
      )) |
    (metadata.progress ? g.YAS_DESKTOP_NOTIFICATION_HAS_PROGRESS : 0) |
    (metadata.replyPlaceholder !== null
      ? g.YAS_DESKTOP_NOTIFICATION_HAS_REPLY
      : 0);
  return { ...value, ...metadata, flags };
}

function decodeDesktopNotificationMetadata(
  extensions: readonly YasExtension[],
  patch: boolean,
): {
  contentImageHash: Uint8Array | null;
  applicationIconHash: Uint8Array | null;
  progress: YasDesktopNotificationProgress | null;
  replyPlaceholder: string | null;
} {
  let contentImageHash: Uint8Array | null = null;
  let applicationIconHash: Uint8Array | null = null;
  let progress: YasDesktopNotificationProgress | null = null;
  let replyPlaceholder: string | null = null;
  for (const extension of extensions) {
    if (patch && extension.value.length === 0) {
      if (desktopNotificationExtensionTags.has(extension.tag)) continue;
    }
    if (extension.tag === g.YAS_DESKTOP_NOTIFICATION_IMAGE_HASH_EXTENSION)
      contentImageHash = decodeDesktopHash(
        extension.value,
        "Desktop notification content image hash",
      );
    else if (
      extension.tag ===
      g.YAS_DESKTOP_NOTIFICATION_APPLICATION_ICON_HASH_EXTENSION
    )
      applicationIconHash = decodeDesktopHash(
        extension.value,
        "Desktop notification application icon hash",
      );
    else if (extension.tag === g.YAS_DESKTOP_NOTIFICATION_PROGRESS_EXTENSION)
      progress = decodeDesktopNotificationProgress(extension.value);
    else if (extension.tag === g.YAS_DESKTOP_NOTIFICATION_REPLY_EXTENSION)
      replyPlaceholder = decodeDesktopNotificationReply(extension.value);
    else if (extension.required)
      throw new YasProtocolError(
        "unknown required Desktop notification extension",
      );
  }
  return {
    contentImageHash,
    applicationIconHash,
    progress,
    replyPlaceholder,
  };
}

function decodeDesktopHash(bytes: Uint8Array, context: string): Uint8Array {
  if (bytes.length !== 32) throw new YasProtocolError(`invalid ${context}`);
  return new Uint8Array(bytes);
}

function encodeDesktopHash(bytes: Uint8Array, context: string): Uint8Array {
  return decodeDesktopHash(bytes, context);
}

function decodeDesktopHashPatch(
  bytes: Uint8Array,
  context: string,
): YasDesktopNotificationFieldPatch<Uint8Array> {
  return bytes.length === 0
    ? { kind: "clear" }
    : { kind: "set", value: decodeDesktopHash(bytes, context) };
}

function decodeDesktopNotificationProgress(
  bytes: Uint8Array,
): YasDesktopNotificationProgress {
  const cursor = new YasCursor(bytes);
  const value = {
    value: cursor.u32("Desktop notification progress value"),
    maximum: cursor.u32("Desktop notification progress maximum"),
  };
  cursor.end("Desktop notification progress");
  if (value.maximum === 0 || value.value > value.maximum)
    throw new YasProtocolError("invalid Desktop notification progress");
  return value;
}

function encodeDesktopNotificationProgress(
  value: YasDesktopNotificationProgress,
): Uint8Array {
  if (
    !Number.isInteger(value.value) ||
    !Number.isInteger(value.maximum) ||
    value.value < 0 ||
    value.maximum <= 0 ||
    value.value > value.maximum ||
    value.maximum > 0xffff_ffff
  )
    throw new YasProtocolError("invalid Desktop notification progress");
  return new YasWriter().u32(value.value).u32(value.maximum).finish();
}

function decodeDesktopNotificationReply(bytes: Uint8Array): string {
  const cursor = new YasCursor(bytes);
  const value = cursor.utf8U16("Desktop notification reply placeholder");
  cursor.end("Desktop notification reply");
  return value;
}

function validateDesktopNotificationPatchExtensions(
  extensions: readonly YasExtension[],
): void {
  let previous = -1;
  for (const extension of extensions) {
    if (extension.tag <= previous)
      throw new YasProtocolError(
        "Desktop notification patch extensions are duplicated or unordered",
      );
    previous = extension.tag;
    if (extension.value.length === 0) {
      if (desktopNotificationExtensionTags.has(extension.tag)) continue;
    }
    if (extension.tag === g.YAS_DESKTOP_NOTIFICATION_IMAGE_HASH_EXTENSION)
      decodeDesktopHash(
        extension.value,
        "Desktop notification content image hash patch",
      );
    else if (
      extension.tag ===
      g.YAS_DESKTOP_NOTIFICATION_APPLICATION_ICON_HASH_EXTENSION
    )
      decodeDesktopHash(
        extension.value,
        "Desktop notification application icon hash patch",
      );
    else if (extension.tag === g.YAS_DESKTOP_NOTIFICATION_PROGRESS_EXTENSION)
      decodeDesktopNotificationProgress(extension.value);
    else if (extension.tag === g.YAS_DESKTOP_NOTIFICATION_REPLY_EXTENSION)
      decodeDesktopNotificationReply(extension.value);
    else if (extension.required)
      throw new YasProtocolError(
        "unknown required Desktop notification patch extension",
      );
  }
}

function mergeDesktopNotificationExtensions(
  previous: readonly YasExtension[],
  patch: readonly YasExtension[],
): YasExtension[] {
  const byTag = new Map(
    previous.map((extension) => [extension.tag, extension]),
  );
  for (const extension of patch) {
    if (
      extension.value.length === 0 &&
      desktopNotificationExtensionTags.has(extension.tag)
    )
      byTag.delete(extension.tag);
    else byTag.set(extension.tag, extension);
  }
  return [...byTag.values()].sort((left, right) => left.tag - right.tag);
}

function validateDesktopNotificationRemoval(
  value: YasDesktopNotificationRemoval,
): void {
  requireHandle(
    value.notificationHandle,
    "Desktop notification removal handle",
  );
  requireRevision(value.revision, "Desktop notification removal revision");
  if (
    value.closeReason < g.YAS_DESKTOP_NOTIFICATION_CLOSED_EXPIRED ||
    value.closeReason > g.YAS_DESKTOP_NOTIFICATION_CLOSED_UNDEFINED
  )
    throw new YasProtocolError("invalid Desktop notification close reason");
}

function sameOptionalHash(
  left: Uint8Array | null,
  right: Uint8Array | null,
): boolean {
  return left === null
    ? right === null
    : right !== null &&
        left.length === right.length &&
        left.every((byte, index) => byte === right[index]);
}

function sameDesktopProgress(
  left: YasDesktopNotificationProgress | null,
  right: YasDesktopNotificationProgress | null,
): boolean {
  return left === null
    ? right === null
    : right !== null &&
        left.value === right.value &&
        left.maximum === right.maximum;
}

function validateTrayAction(value: YasDesktopTrayAction): void {
  requireHandle(value.trayHandle, "Desktop tray handle");
  requireRevision(value.trayRevision, "Desktop tray revision");
  requireOperationId(value.operationId, "Desktop TRAY_ACTION");
  const menuItem = value.actionKind === g.YAS_DESKTOP_TRAY_ACTION_MENU_ITEM;
  const scroll = value.actionKind === g.YAS_DESKTOP_TRAY_ACTION_SCROLL;
  if (
    value.actionKind > g.YAS_DESKTOP_TRAY_ACTION_MENU_ITEM ||
    value.flags & ~g.YAS_DESKTOP_TRAY_ACTION_FLAGS_MASK ||
    (!scroll && (value.flags !== 0 || value.value !== 0)) ||
    (scroll && value.value === 0) ||
    menuItem !== (value.itemHandle !== 0n) ||
    menuItem !== (value.menuRevision !== 0n)
  )
    throw new YasProtocolError("invalid Desktop tray action shape");
}

function validateNotificationAction(value: YasDesktopNotificationAction): void {
  requireHandle(value.notificationHandle, "Desktop notification handle");
  requireRevision(value.revision, "Desktop notification revision");
  requireOperationId(value.operationId, "Desktop NOTIFICATION_ACTION");
  const action = value.actionKind === g.YAS_DESKTOP_NOTIFICATION_ACTION_ACTION;
  if (
    value.actionKind > g.YAS_DESKTOP_NOTIFICATION_ACTION_DISMISS ||
    action !== (value.actionHandle !== 0n) ||
    (!action && value.reply.length !== 0)
  )
    throw new YasProtocolError("invalid Desktop notification action shape");
}

function requireOperationId(value: Uint8Array, context: string): void {
  if (value.length !== 16 || value.every((byte) => byte === 0))
    throw new YasProtocolError(
      `${context} operation ID is zero or not 16 bytes`,
    );
}

function requireHandle(value: bigint, context: string): void {
  if (value === 0n) throw new YasProtocolError(`${context} is zero`);
}

function requireRevision(value: bigint, context: string): void {
  if (value === 0n) throw new YasProtocolError(`${context} is zero`);
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
