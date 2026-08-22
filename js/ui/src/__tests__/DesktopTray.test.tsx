import {
  TRAY_HAS_MENU,
  type YasConnectionSnapshot,
  type YasWorkspace,
} from "@yas-run/core";
import { render } from "solid-js/web";
import { createSignal } from "solid-js";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DesktopStore } from "../../../core/src/desktopModel";
import * as g from "../../../core/src/yas/generated";
import { YasNativeDesktopClientLifecycle } from "../../../core/src/yas/nativeDesktopMedia";
import { DesktopChrome } from "../DesktopChrome";
import { t } from "../i18n";
import { darkTheme, uiScale } from "../theme";

let dispose: (() => void) | undefined;
afterEach(() => {
  dispose?.();
  dispose = undefined;
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

function mountTray(readOnly = false, compact = false) {
  const store = new DesktopStore();
  store.replaceNative(
    [
      {
        trayId: 7n,
        revision: 3n,
        status: 1,
        category: 0,
        flags: TRAY_HAS_MENU,
        appId: "Slack",
        title: "Slack",
        tooltipTitle: "Slack",
        tooltipBody: "",
        icon: { width: 0, height: 0, png: new Uint8Array() },
      },
    ],
    [],
  );
  const desktop = {
    catalog: {
      snapshot: { trays: [{ trayHandle: 7n, revision: 3n, menuRevision: 5n }] },
    },
    getMenu: vi.fn(async () => ({
      menu: async () => ({
        trayHandle: 7n,
        trayRevision: 3n,
        menuRevision: 5n,
        nodes: [
          {
            nodeHandle: 10n,
            parentHandle: 0n,
            actionHandle: 0n,
            kind: g.YAS_DESKTOP_MENU_NODE_ROOT,
            flags: g.YAS_DESKTOP_MENU_VISIBLE,
            position: 0,
            label: "",
          },
          {
            nodeHandle: 11n,
            parentHandle: 10n,
            actionHandle: 21n,
            kind: g.YAS_DESKTOP_MENU_NODE_SUBMENU,
            flags: g.YAS_DESKTOP_MENU_VISIBLE | g.YAS_DESKTOP_MENU_ENABLED,
            position: 0,
            label: "More",
          },
          {
            nodeHandle: 12n,
            parentHandle: 11n,
            actionHandle: 22n,
            kind: g.YAS_DESKTOP_MENU_NODE_ITEM,
            flags: g.YAS_DESKTOP_MENU_VISIBLE | g.YAS_DESKTOP_MENU_ENABLED,
            position: 0,
            label: "Preferences",
          },
        ],
      }),
    })),
    trayAction: vi.fn().mockResolvedValue(undefined),
  };
  const lifecycle = Object.create(YasNativeDesktopClientLifecycle.prototype);
  Object.assign(lifecycle, { desktop, options: { desktopStore: store } });
  store.setNativeController(lifecycle.desktopController());
  const connection = { desktopStore: store };
  const workspace = {
    getConnection: () => connection,
  } as unknown as YasWorkspace;
  const [connections, setConnections] = createSignal([
    { id: "dev", supportsDesktop: true } as YasConnectionSnapshot,
  ]);
  dispose = render(
    () => (
      <DesktopChrome
        workspace={workspace}
        connections={connections()}
        connectionLabels={new Map()}
        readOnlyConnections={new Set(readOnly ? ["dev"] : [])}
        theme={darkTheme}
        scale={uiScale(13)}
        compact={compact}
      />
    ),
    document.body,
  );
  if (compact) {
    document
      .querySelector<HTMLButtonElement>(
        `button[title="${t("desktop.trayOverflow")}"]`,
      )!
      .click();
  }
  const icon = document.querySelector<HTMLButtonElement>(
    'button[title="Slack"]',
  )!;
  vi.spyOn(icon, "getBoundingClientRect").mockReturnValue(
    new DOMRect(0, 0, 40, 40),
  );
  const refresh = () =>
    setConnections((items) => items.map((item) => ({ ...item })));
  return { desktop, icon, refresh, store };
}

function touch(icon: HTMLElement, type: string, x = 20) {
  const point = { identifier: 1, clientX: x, clientY: 20 };
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperties(event, {
    touches: {
      value: type === "touchend" || type === "touchcancel" ? [] : [point],
    },
    changedTouches: { value: [point] },
  });
  icon.dispatchEvent(event);
}

describe("desktop tray interaction", () => {
  it("renders native top-level and nested entries after right click and dispatches selection", async () => {
    const { desktop, icon } = mountTray();
    icon.dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
    );
    await vi.waitFor(() =>
      expect(document.querySelector('[role="menu"]')?.textContent).toContain(
        "More",
      ),
    );
    const preference = Array.from(
      document.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
    ).find((button) => button.textContent?.includes("Preferences"));
    expect(preference).toBeDefined();
    preference!.click();
    await vi.waitFor(() => expect(desktop.trayAction).toHaveBeenCalledOnce());
    expect(desktop.trayAction).toHaveBeenCalledWith(
      expect.objectContaining({
        trayHandle: 7n,
        menuRevision: 5n,
        actionKind: g.YAS_DESKTOP_TRAY_ACTION_MENU_ITEM,
        itemHandle: 22n,
      }),
    );
    expect(document.querySelector('[role="menu"]')).toBeNull();
  });

  it("keeps ordinary left click as activation", async () => {
    const { desktop, icon } = mountTray();
    icon.click();
    await vi.waitFor(() => expect(desktop.trayAction).toHaveBeenCalledOnce());
    expect(desktop.trayAction).toHaveBeenCalledWith(
      expect.objectContaining({
        actionKind: g.YAS_DESKTOP_TRAY_ACTION_ACTIVATE,
      }),
    );
    expect(desktop.getMenu).not.toHaveBeenCalled();
  });

  it("opens an advertised menu on a touch tap without also activating", async () => {
    const { desktop, icon } = mountTray();
    const press = new MouseEvent("pointerdown", { bubbles: true });
    Object.defineProperty(press, "pointerType", { value: "touch" });
    icon.dispatchEvent(press);
    icon.click();
    await vi.waitFor(() =>
      expect(document.querySelector('[role="menu"]')?.textContent).toContain(
        "Preferences",
      ),
    );
    expect(desktop.trayAction).not.toHaveBeenCalled();
  });

  it.each([false, true])(
    "opens from touch events without a pointer or compatibility click (overflow: %s)",
    async (compact) => {
      const { desktop, icon } = mountTray(false, compact);
      touch(icon, "touchstart");
      touch(icon, "touchend");
      await vi.waitFor(() =>
        expect(document.querySelector('[role="menu"]')?.textContent).toContain(
          "Preferences",
        ),
      );
      icon.dispatchEvent(
        new MouseEvent("click", {
          bubbles: true,
          detail: 1,
          clientX: 20,
          clientY: 20,
        }),
      );
      expect(desktop.getMenu).toHaveBeenCalledOnce();
      expect(desktop.trayAction).not.toHaveBeenCalled();
    },
  );

  it.each([
    [false, false],
    [false, true],
    [true, false],
    [true, true],
  ])(
    "keeps a touch across connection updates (overflow: %s, context menu: %s)",
    async (compact, contextMenu) => {
      const { desktop, icon, refresh } = mountTray(false, compact);
      const press = new MouseEvent("pointerdown", { bubbles: true });
      Object.defineProperty(press, "pointerType", { value: "touch" });
      icon.dispatchEvent(press);
      touch(icon, "touchstart");
      refresh();
      expect(document.querySelector('button[title="Slack"]')).toBe(icon);
      if (contextMenu)
        icon.dispatchEvent(
          new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
        );
      touch(icon, "touchend");
      await vi.waitFor(() =>
        expect(document.querySelector('[role="menu"]')?.textContent).toContain(
          "Preferences",
        ),
      );
      expect(desktop.getMenu).toHaveBeenCalledOnce();
      expect(desktop.trayAction).not.toHaveBeenCalled();
    },
  );

  it("updates a retained icon and uses the current tray revision", async () => {
    const { desktop, icon, refresh, store } = mountTray();
    touch(icon, "touchstart");
    store.replaceNative(
      [{ ...store.tray.get(7n)!, revision: 4n, tooltipTitle: "New title" }],
      [],
    );
    desktop.catalog.snapshot.trays[0].revision = 4n;
    refresh();
    expect(document.querySelector('button[title="New title"]')).toBe(icon);
    touch(icon, "touchend");
    await vi.waitFor(() =>
      expect(document.querySelector('[role="menu"]')?.textContent).toContain(
        "Preferences",
      ),
    );
    expect(desktop.getMenu).toHaveBeenCalledWith(7n, 4n, 5n);
  });

  it.each(["swipe", "cancel", "remove"])(
    "does not open a menu after a touch %s",
    (end) => {
      const { desktop, icon, refresh, store } = mountTray();
      touch(icon, "touchstart");
      if (end === "swipe") touch(icon, "touchmove", 80);
      if (end === "cancel") touch(icon, "touchcancel");
      if (end === "remove") {
        store.replaceNative([], []);
        refresh();
        expect(icon.isConnected).toBe(false);
      }
      touch(icon, "touchend");
      expect(desktop.getMenu).not.toHaveBeenCalled();
      expect(desktop.trayAction).not.toHaveBeenCalled();
    },
  );

  it("does not send activation or menu requests from read-only views", () => {
    const { desktop, icon } = mountTray(true);
    expect(icon.disabled).toBe(true);
    icon.click();
    icon.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    expect(desktop.trayAction).not.toHaveBeenCalled();
    expect(desktop.getMenu).not.toHaveBeenCalled();
  });
});
