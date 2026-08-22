import { describe, expect, it } from "vitest";
import type { LayoutNode, LayoutSplit } from "@yas-run/core/layout";
import { enumeratePanes } from "@yas-run/core/layout";
import { moveViewIntoStack, moveViewToEdge } from "../layout/viewMovement";

const leaf = (): LayoutNode => ({ type: "leaf" });

describe("view movement", () => {
  it("moves a view into its neighbor as the active tab", () => {
    const root: LayoutSplit = {
      type: "split",
      direction: "horizontal",
      children: [
        { node: leaf(), weight: 1 },
        { node: leaf(), weight: 1 },
      ],
    };
    const moved = moveViewIntoStack(root, { "0": "a", "1": "b" }, "0", "1");
    expect(moved?.root).toMatchObject({ type: "split", direction: "tabs" });
    expect(moved?.assignments).toEqual({ "0": "b", "1": "a" });
    expect(moved?.focusedPaneId).toBe("1");
  });

  it("removes only the active source tab when the stack has others", () => {
    const tabs: LayoutSplit = {
      type: "split",
      direction: "tabs",
      children: [
        { node: leaf(), weight: 1 },
        { node: leaf(), weight: 1 },
      ],
    };
    const root: LayoutSplit = {
      type: "split",
      direction: "horizontal",
      children: [
        { node: tabs, weight: 1 },
        { node: leaf(), weight: 1 },
      ],
    };
    const moved = moveViewIntoStack(
      root,
      { "0.0": "keep", "0.1": "move", "1": "target" },
      "0.1",
      "1",
    );
    expect(Object.values(moved?.assignments ?? {})).toEqual([
      "keep",
      "target",
      "move",
    ]);
    expect(moved?.assignments[moved.focusedPaneId]).toBe("move");
  });

  it("creates a populated stack at an empty edge", () => {
    const root: LayoutSplit = {
      type: "split",
      direction: "horizontal",
      children: [
        { node: leaf(), weight: 1 },
        { node: leaf(), weight: 1 },
      ],
    };
    const moved = moveViewToEdge(root, { "0": "a", "1": "b" }, "1", "left");
    expect(moved?.root).toMatchObject({
      type: "split",
      direction: "horizontal",
    });
    expect(moved?.assignments).toEqual({ "0": "b", "1": "a" });
    expect(moved?.focusedPaneId).toBe("0");
    expect(enumeratePanes(moved!.root)).toHaveLength(2);
  });

  it("does not manufacture an edge stack for the only view", () => {
    expect(moveViewToEdge(leaf(), { "0": "a" }, "0", "right")).toBeNull();
  });
});
