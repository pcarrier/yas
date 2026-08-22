import { describe, expect, it, vi } from "vitest";
import {
  DesktopStore,
  MENU_NODE_ENABLED,
  MENU_NODE_SEPARATOR,
  MENU_NODE_SUBMENU,
  MENU_NODE_VISIBLE,
  type NativeDesktopController,
  type TrayMenu,
} from "../desktopModel";
import {
  decodeDesktopMenuTree,
  encodeDesktopMenuTree,
  type YasDesktopMenuNode,
  type YasDesktopMenuTree,
} from "../yas/desktop";
import * as g from "../yas/generated";
import { YasNativeDesktopClientLifecycle } from "../yas/nativeDesktopMedia";

function fixture() {
  const node = (
    nodeHandle: bigint,
    parentHandle: bigint,
    overrides: Partial<YasDesktopMenuNode> = {},
  ): YasDesktopMenuNode => ({
    nodeHandle,
    parentHandle,
    actionHandle: nodeHandle + 100n,
    kind: g.YAS_DESKTOP_MENU_NODE_ITEM,
    flags: g.YAS_DESKTOP_MENU_VISIBLE | g.YAS_DESKTOP_MENU_ENABLED,
    position: 0,
    label: "Open app",
    shortcut: "",
    iconHash: new Uint8Array(32),
    extensions: [],
    ...overrides,
  });
  // Handles are opaque u64s, and action handles need not equal node handles.
  const root = 2n ** 60n;
  const tree: YasDesktopMenuTree = {
    trayHandle: 7n,
    trayRevision: 3n,
    menuRevision: 5n,
    extensions: [],
    nodes: [
      node(root, 0n, { kind: g.YAS_DESKTOP_MENU_NODE_ROOT, actionHandle: 0n }),
      node(root + 1n, root),
      node(root + 2n, root, {
        kind: g.YAS_DESKTOP_MENU_NODE_SUBMENU,
        position: 1,
        label: "More",
      }),
      node(root + 3n, root + 2n, { label: "Preferences" }),
      node(root + 4n, root, {
        kind: g.YAS_DESKTOP_MENU_NODE_SEPARATOR,
        position: 2,
        actionHandle: 0n,
        label: "",
      }),
      node(root + 5n, root, {
        position: 3,
        flags: g.YAS_DESKTOP_MENU_VISIBLE,
        actionHandle: 0n,
        label: "Running",
      }),
    ],
  };
  const store = new DesktopStore();
  const menus: TrayMenu[] = [];
  store.onTrayMenu((menu) => menus.push(menu));
  const desktop = {
    catalog: {
      snapshot: {
        trays: [{ trayHandle: 7n, revision: 3n, menuRevision: 5n }],
      },
    },
    getMenu: vi.fn(async () => ({
      menu: async () => decodeDesktopMenuTree(encodeDesktopMenuTree(tree)),
    })),
    trayAction: vi.fn().mockResolvedValue(undefined),
  };
  const lifecycle = Object.create(YasNativeDesktopClientLifecycle.prototype);
  Object.assign(lifecycle, { desktop, options: { desktopStore: store } });
  const controller: NativeDesktopController = lifecycle.desktopController();
  store.setNativeController(controller);
  return { store, menus, desktop, root };
}

describe("native tray menus", () => {
  it("projects the hidden root and nested parents into presentation IDs", async () => {
    const { store, menus, desktop, root } = fixture();
    store.openMenu(7n);
    await vi.waitFor(() => expect(menus).toHaveLength(1));

    expect(desktop.getMenu).toHaveBeenCalledWith(7n, 3n, 5n);
    const menu = menus[0]!;
    expect(menu.nodes.map(({ id, parentId }) => ({ id, parentId }))).toEqual([
      { id: root + 101n, parentId: 0n },
      { id: root + 102n, parentId: 0n },
      { id: root + 103n, parentId: root + 102n },
      { id: root + 4n, parentId: 0n },
      { id: root + 5n, parentId: 0n },
    ]);
    expect(menu.nodes[1]!.flags & MENU_NODE_SUBMENU).toBeTruthy();
    expect(menu.nodes[3]!.flags & MENU_NODE_SEPARATOR).toBeTruthy();
    expect(menu.nodes[4]!.flags & MENU_NODE_VISIBLE).toBeTruthy();
    expect(menu.nodes[4]!.flags & MENU_NODE_ENABLED).toBe(0);
  });

  it("dispatches the selected menu action with its menu revision", async () => {
    const { store, menus, desktop, root } = fixture();
    store.openMenu(7n);
    await vi.waitFor(() => expect(menus).toHaveLength(1));
    store.clickMenuItem(7n, menus[0]!.menuRevision, menus[0]!.nodes[2]!.id);
    await vi.waitFor(() => expect(desktop.trayAction).toHaveBeenCalledOnce());
    expect(desktop.trayAction).toHaveBeenCalledWith(
      expect.objectContaining({
        trayHandle: 7n,
        trayRevision: 3n,
        menuRevision: 5n,
        actionKind: g.YAS_DESKTOP_TRAY_ACTION_MENU_ITEM,
        itemHandle: root + 103n,
      }),
    );
  });

  it("keeps activation and secondary activation separate from menu requests", async () => {
    const { store, desktop } = fixture();
    store.activate(7n);
    store.secondaryActivate(7n);
    await vi.waitFor(() => expect(desktop.trayAction).toHaveBeenCalledTimes(2));
    expect(
      desktop.trayAction.mock.calls.map(([action]) => action.actionKind),
    ).toEqual([
      g.YAS_DESKTOP_TRAY_ACTION_ACTIVATE,
      g.YAS_DESKTOP_TRAY_ACTION_SECONDARY_ACTIVATE,
    ]);
    expect(desktop.getMenu).not.toHaveBeenCalled();
  });
});
