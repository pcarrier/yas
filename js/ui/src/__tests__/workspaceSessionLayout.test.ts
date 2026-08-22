import type { LayoutNode, WorkspaceLayout } from "@yas-run/core/layout";
import { describe, expect, it } from "vitest";
import {
  connectionAwaitingWorkspaceRestore,
  loadActiveLayout,
  loadActiveLayoutState,
  parseWorkspaceRef,
  ptyIdForWorkspaceRef,
  surfaceIdForWorkspaceRef,
  surfaceWorkspaceRef,
  surfaceWorkspaceRefForId,
  saveActiveLayout,
  saveActiveLayoutState,
  tabWorkspaceRef,
  terminalWorkspaceRef,
  terminalWorkspaceRefForPtyId,
} from "../layout/store";

describe("workspace-session stable pane references", () => {
  it("restores an active layout only from device storage", () => {
    history.replaceState(
      null,
      "",
      "/#session=123e4567-e89b-42d3-a456-426614174000&debug",
    );
    localStorage.setItem(
      "yas.layout",
      JSON.stringify({
        name: "Stored",
        root: {
          type: "split",
          direction: "horizontal",
          children: [
            { node: { type: "leaf" }, weight: 1 },
            { node: { type: "leaf" }, weight: 1 },
          ],
        },
      }),
    );

    try {
      expect(loadActiveLayout()).toMatchObject({
        name: "Stored",
        root: {
          type: "split",
          direction: "horizontal",
          children: [
            { node: { type: "leaf" }, weight: 1 },
            { node: { type: "leaf" }, weight: 1 },
          ],
        },
      });
      localStorage.removeItem("yas.layout");
      expect(loadActiveLayout()).toBeNull();
    } finally {
      localStorage.removeItem("yas.layout");
      history.replaceState(null, "", "/");
    }
  });

  it("restores device-local stable pane assignments with the layout", () => {
    const layout = {
      ...({
        name: "Test layout",
        root: {
          type: "split",
          direction: "horizontal",
          children: [
            { node: { type: "leaf" }, weight: 1 },
            { node: { type: "leaf" }, weight: 1 },
          ],
        } as LayoutNode,
      } as WorkspaceLayout),
      name: "Local",
    };
    saveActiveLayoutState(
      layout,
      {
        "0": "terminal:local:11",
        "1": "surface:remote:22",
      },
      "1",
    );
    // LayoutContainer writes the same tree while assignments are published by
    // a separate reactive effect. That intermediate write must not erase the
    // stable identities it is about to restore.
    saveActiveLayout(layout);

    try {
      expect(loadActiveLayoutState()).toMatchObject({
        layout: {
          name: "Local",
          root: {
            type: "split",
            direction: "horizontal",
            children: [
              { node: { type: "leaf" }, weight: 1 },
              { node: { type: "leaf" }, weight: 1 },
            ],
          },
        },
        assignments: {
          "0": "terminal:local:11",
          "1": "surface:remote:22",
        },
        focusedPaneId: "1",
      });
    } finally {
      localStorage.removeItem("yas.layout");
    }
  });

  it("waits only for an active initial handshake", () => {
    expect(
      connectionAwaitingWorkspaceRestore({
        ready: false,
        status: "authenticating",
      }),
    ).toBe(true);
    expect(
      connectionAwaitingWorkspaceRestore({ ready: false, status: "error" }),
    ).toBe(false);
    expect(
      connectionAwaitingWorkspaceRestore({
        ready: false,
        status: "disconnected",
      }),
    ).toBe(false);
    expect(connectionAwaitingWorkspaceRestore(null)).toBe(false);
  });

  it("round-trips terminals, surfaces, and server tabs", () => {
    expect(parseWorkspaceRef(terminalWorkspaceRef("build:west", 42n))).toEqual({
      kind: "terminal",
      connectionId: "build:west",
      terminalHandle: 42n,
    });
    expect(parseWorkspaceRef(surfaceWorkspaceRef("desktop", 7n))).toEqual({
      kind: "surface",
      connectionId: "desktop",
      surfaceHandle: 7n,
    });
    expect(parseWorkspaceRef(tabWorkspaceRef("dev", "0k3vq8za"))).toEqual({
      kind: "tab",
      connectionId: "dev",
      tabId: "0k3vq8za",
    });
    expect(parseWorkspaceRef(tabWorkspaceRef("日本:西", "42:β"))).toEqual({
      kind: "tab",
      connectionId: "日本:西",
      tabId: "42:β",
    });
  });

  it("round-trips valid long Unicode remote names after percent expansion", () => {
    const connectionId = "界".repeat(200);
    const encoded = terminalWorkspaceRef(connectionId, 42n);

    expect(encoded.length).toBeGreaterThan(1_024);
    expect(parseWorkspaceRef(encoded)).toEqual({
      kind: "terminal",
      connectionId,
      terminalHandle: 42n,
    });
  });

  it("rejects connection names beyond the persisted UTF-8 byte limit", () => {
    const connectionId = "界".repeat(342);
    const encoded = `terminal:${encodeURIComponent(connectionId)}:42`;

    expect(() => terminalWorkspaceRef(connectionId, 42n)).toThrow(RangeError);
    expect(parseWorkspaceRef(encoded)).toBeNull();
  });

  it("persists an opaque native terminal handle without an alias", () => {
    const stored = terminalWorkspaceRefForPtyId("build:west", 81n);
    expect(stored).toBe("terminal:build%3Awest:81");
    const parsed = parseWorkspaceRef(stored);
    expect(parsed?.kind).toBe("terminal");
    if (parsed?.kind !== "terminal") throw new Error("expected terminal ref");
    expect(ptyIdForWorkspaceRef(parsed)).toBe(81n);
  });

  it("persists an opaque native surface handle without an alias", () => {
    const stored = surfaceWorkspaceRefForId(
      "日本:西",
      18_446_744_073_709_551_614n,
    );
    expect(stored).toBe(
      "surface:%E6%97%A5%E6%9C%AC%3A%E8%A5%BF:18446744073709551614",
    );
    const parsed = parseWorkspaceRef(stored);
    expect(parsed?.kind).toBe("surface");
    if (parsed?.kind !== "surface") throw new Error("expected surface ref");
    expect(surfaceIdForWorkspaceRef(parsed)).toBe(18_446_744_073_709_551_614n);
  });

  it("rejects retired browser-local numeric aliases", () => {
    expect(parseWorkspaceRef("pty:old:9")).toBeNull();
    expect(parseWorkspaceRef("surface-id:old:9")).toBeNull();
  });

  it("rejects malformed and unsafe numeric references", () => {
    expect(parseWorkspaceRef("local:3")).toBeNull();
    expect(parseWorkspaceRef("surface::3")).toBeNull();
    expect(parseWorkspaceRef("surface:local:-1")).toBeNull();
    expect(parseWorkspaceRef("surface:local:18446744073709551616")).toBeNull();
    expect(parseWorkspaceRef("surface-id:local:65536")).toBeNull();
    expect(parseWorkspaceRef("pty:local:9007199254740992")).toBeNull();
    expect(parseWorkspaceRef("terminal:local:18446744073709551616")).toBeNull();
  });
});
