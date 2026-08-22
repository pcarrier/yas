import { Notifier, type ReactiveStore } from "./reactive";

export const DESKTOP_SUBSCRIBE_TRAY = 1 << 0;
export const DESKTOP_SUBSCRIBE_NOTIFICATIONS = 1 << 1;
export const DESKTOP_SUBSCRIBE_ALL =
  DESKTOP_SUBSCRIBE_TRAY | DESKTOP_SUBSCRIBE_NOTIFICATIONS;

export const TRAY_STATUS_PASSIVE = 0;
export const TRAY_STATUS_ACTIVE = 1;
export const TRAY_STATUS_NEEDS_ATTENTION = 2;
export const TRAY_HAS_MENU = 1 << 0;
export const TRAY_ITEM_IS_MENU = 1 << 1;

export const TRAY_MENU_OK = 0;
export const TRAY_MENU_NONE = 1;
export const TRAY_MENU_UNAVAILABLE = 2;
export const TRAY_MENU_STALE = 3;

export const MENU_NODE_VISIBLE = 1 << 0;
export const MENU_NODE_ENABLED = 1 << 1;
export const MENU_NODE_SEPARATOR = 1 << 2;
export const MENU_NODE_SUBMENU = 1 << 3;
export const MENU_NODE_CHECKMARK = 1 << 4;
export const MENU_NODE_RADIO = 1 << 5;

export const NOTIFICATION_RESIDENT = 1 << 0;
export const NOTIFICATION_TRANSIENT = 1 << 1;

export const NOTIFICATION_CLOSED_EXPIRED = 1;
export const NOTIFICATION_CLOSED_DISMISSED = 2;
export const NOTIFICATION_CLOSED_BY_CALLER = 3;
export const NOTIFICATION_CLOSED_UNDEFINED = 4;

export interface DesktopImage {
  width: number;
  height: number;
  png: Uint8Array;
}

/** Opaque native YAS Desktop resource handle. */
export type DesktopId = bigint;
/** Native YAS Desktop catalogue revision. */
export type DesktopRevision = bigint;

export interface TrayItem {
  trayId: DesktopId;
  revision: DesktopRevision;
  status: number;
  category: number;
  flags: number;
  appId: string;
  title: string;
  tooltipTitle: string;
  tooltipBody: string;
  icon: DesktopImage;
}

export interface TrayMenuNode {
  id: DesktopId;
  parentId: DesktopId;
  position: number;
  flags: number;
  toggleState: number;
  label: string;
  icon: DesktopImage;
}

export interface TrayMenu {
  trayId: DesktopId;
  trayRevision: DesktopRevision;
  menuRevision: DesktopRevision;
  status: number;
  nodes: readonly TrayMenuNode[];
}

export interface NotificationAction {
  key: string;
  label: string;
}

export interface DesktopNotification {
  notificationId: DesktopId;
  revision: DesktopRevision;
  urgency: number;
  flags: number;
  timeoutMs: number;
  appName: string;
  desktopEntry: string;
  summary: string;
  body: string;
  icon: DesktopImage;
  image: DesktopImage;
  actions: readonly NotificationAction[];
}

export interface NativeDesktopController {
  subscribe(flags: number): void | Promise<void>;
  activate(trayId: DesktopId): void | Promise<void>;
  secondaryActivate(trayId: DesktopId): void | Promise<void>;
  openMenu(
    trayId: DesktopId,
    menuRevision: DesktopRevision,
    parentId: DesktopId,
  ): void | Promise<void>;
  scroll(
    trayId: DesktopId,
    delta: number,
    horizontal: boolean,
  ): void | Promise<void>;
  clickMenuItem(
    trayId: DesktopId,
    menuRevision: DesktopRevision,
    itemId: DesktopId,
  ): void | Promise<void>;
  invokeDefault(
    notificationId: DesktopId,
    revision: DesktopRevision,
  ): void | Promise<void>;
  invokeAction(
    notificationId: DesktopId,
    revision: DesktopRevision,
    key: string,
  ): void | Promise<void>;
  dismiss(
    notificationId: DesktopId,
    revision: DesktopRevision,
  ): void | Promise<void>;
}

/** Browser presentation state driven exclusively by the native YAS Desktop family. */
export class DesktopStore implements ReactiveStore {
  readonly #notifier = new Notifier();
  readonly #tray = new Map<DesktopId, TrayItem>();
  readonly #notifications = new Map<DesktopId, DesktopNotification>();
  #native: NativeDesktopController | null = null;
  readonly #raised = new Set<(notification: DesktopNotification) => void>();
  readonly #menus = new Set<(menu: TrayMenu) => void>();

  get revision(): number {
    return this.#notifier.revision;
  }

  subscribe = (listener: () => void): (() => void) =>
    this.#notifier.subscribe(listener);

  get tray(): ReadonlyMap<DesktopId, TrayItem> {
    return this.#tray;
  }

  get notifications(): ReadonlyMap<DesktopId, DesktopNotification> {
    return this.#notifications;
  }

  setNativeController(controller: NativeDesktopController | null): void {
    this.#native = controller;
  }

  replaceNative(
    tray: readonly TrayItem[],
    notifications: readonly DesktopNotification[],
    replay = false,
  ): void {
    const raised = replay
      ? []
      : notifications.filter((item) => {
          const previous = this.#notifications.get(item.notificationId);
          return previous?.revision !== item.revision;
        });
    this.#tray.clear();
    for (const item of tray) this.#tray.set(item.trayId, item);
    this.#notifications.clear();
    for (const item of notifications)
      this.#notifications.set(item.notificationId, item);
    this.#notifier.emit();
    for (const item of raised)
      for (const listener of [...this.#raised]) listener(item);
  }

  publishNativeMenu(menu: TrayMenu): void {
    for (const listener of [...this.#menus]) listener(menu);
  }

  onNotificationRaised(
    listener: (notification: DesktopNotification) => void,
  ): () => void {
    this.#raised.add(listener);
    return () => this.#raised.delete(listener);
  }

  onTrayMenu(listener: (menu: TrayMenu) => void): () => void {
    this.#menus.add(listener);
    return () => this.#menus.delete(listener);
  }

  subscribeDesktop(flags = DESKTOP_SUBSCRIBE_ALL): void {
    void this.#native?.subscribe(flags);
  }

  activate(trayId: DesktopId): void {
    void this.#native?.activate(trayId);
  }

  secondaryActivate(trayId: DesktopId): void {
    void this.#native?.secondaryActivate(trayId);
  }

  openMenu(
    trayId: DesktopId,
    menuRevision: DesktopRevision = 0n,
    parentId: DesktopId = 0n,
  ): void {
    void this.#native?.openMenu(trayId, menuRevision, parentId);
  }

  scroll(trayId: DesktopId, delta: number, horizontal = false): void {
    void this.#native?.scroll(trayId, delta, horizontal);
  }

  clickMenuItem(
    trayId: DesktopId,
    menuRevision: DesktopRevision,
    itemId: DesktopId,
  ): void {
    void this.#native?.clickMenuItem(trayId, menuRevision, itemId);
  }

  invokeDefault(notificationId: DesktopId, revision: DesktopRevision): void {
    void this.#native?.invokeDefault(notificationId, revision);
  }

  invokeAction(
    notificationId: DesktopId,
    revision: DesktopRevision,
    key: string,
  ): void {
    if (key) void this.#native?.invokeAction(notificationId, revision, key);
  }

  dismiss(notificationId: DesktopId, revision: DesktopRevision): void {
    void this.#native?.dismiss(notificationId, revision);
  }

  reset(): void {
    const changed = this.#tray.size > 0 || this.#notifications.size > 0;
    this.#tray.clear();
    this.#notifications.clear();
    if (changed) this.#notifier.emit();
  }
}
