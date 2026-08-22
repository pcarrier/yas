import type { LayoutNode, WorkspaceLayout } from "@yas-run/core/layout";
import { describe, expect, it } from "vitest";
import { surfaceAssignment } from "@yas-run/core/layout";
import { enumeratePanes } from "../layout/store";
import { insertTabAtPane, isParkedTabDropTarget } from "../layout/tabGrouping";

describe("sidebar tab grouping", () => {
  it("does not use parked surface previews as tab targets", () => {
    expect(isParkedTabDropTarget(surfaceAssignment("dev", 7n))).toBe(false);
    expect(isParkedTabDropTarget("terminal-session-id")).toBe(true);
    expect(isParkedTabDropTarget("editor:/src/yas/README.md")).toBe(true);
  });

  it("turns one fullscreen leaf into two tabs without changing its pane id", () => {
    const root = (
      {
        name: "Test layout",
        root: { type: "leaf" } as LayoutNode,
      } as WorkspaceLayout
    ).root;
    const inserted = insertTabAtPane(root, "0");

    expect(inserted).not.toBeNull();
    expect(inserted!.sourcePaneId).toBe("0");
    expect(inserted!.newPaneId).toBe("1");
    expect(inserted!.root).toMatchObject({
      type: "split",
      direction: "tabs",
    });
    expect(enumeratePanes(inserted!.root).map((pane) => pane.id)).toEqual([
      "0",
      "1",
    ]);
  });

  it("nests tabs inside one floating frame and leaves its siblings alone", () => {
    const root = (
      {
        name: "Test layout",
        root: {
          type: "split",
          direction: "workspace",
          children: [
            {
              node: { type: "leaf" },
              weight: 1,
              rect: { x: 5, y: 5, width: 40, height: 40 },
            },
            {
              node: { type: "leaf" },
              weight: 1,
              rect: { x: 55, y: 5, width: 40, height: 40 },
            },
          ],
        } as LayoutNode,
      } as WorkspaceLayout
    ).root;
    const inserted = insertTabAtPane(root, "0");

    expect(inserted).not.toBeNull();
    expect(inserted!.sourcePaneId).toBe("0.0");
    expect(inserted!.newPaneId).toBe("0.1");
    expect(enumeratePanes(inserted!.root).map((pane) => pane.id)).toEqual([
      "0.0",
      "0.1",
      "1",
    ]);
    expect(inserted!.root).toMatchObject({
      type: "split",
      direction: "workspace",
      children: [
        {
          rect: { x: 5, y: 5, width: 40, height: 40 },
          node: { type: "split", direction: "tabs" },
        },
        { rect: { x: 55, y: 5, width: 40, height: 40 } },
      ],
    });
  });

  it("appends to an existing tab bar instead of nesting another one", () => {
    const root = (
      {
        name: "Test layout",
        root: {
          type: "split",
          direction: "tabs",
          children: [
            { node: { type: "leaf" }, weight: 1 },
            { node: { type: "leaf" }, weight: 1 },
          ],
        } as LayoutNode,
      } as WorkspaceLayout
    ).root;
    const inserted = insertTabAtPane(root, "1");

    expect(inserted).not.toBeNull();
    expect(inserted!.sourcePaneId).toBe("1");
    expect(inserted!.newPaneId).toBe("2");
    expect(enumeratePanes(inserted!.root).map((pane) => pane.id)).toEqual([
      "0",
      "1",
      "2",
    ]);
  });

  it("appends to an existing stacking container without changing its layout", () => {
    const root = (
      {
        name: "Test layout",
        root: {
          type: "split",
          direction: "stacking",
          children: [
            { node: { type: "leaf" }, weight: 1 },
            { node: { type: "leaf" }, weight: 1 },
          ],
        } as LayoutNode,
      } as WorkspaceLayout
    ).root;
    const inserted = insertTabAtPane(root, "1");

    expect(inserted!.root).toMatchObject({
      type: "split",
      direction: "stacking",
      children: [{}, {}, {}],
    });
    expect(inserted!.newPaneId).toBe("2");
  });
});
