import { describe, expect, it } from "vitest";
import type { LayoutNode, LayoutSplit } from "@yas-run/core/layout";
import {
  balanceLayout,
  resizePaneInDirection,
} from "../layout/directionalResize";

const leaf = (): LayoutNode => ({ type: "leaf" });
const split = (
  direction: "horizontal" | "vertical" | "tabs",
  weights: number[],
): LayoutSplit => ({
  type: "split",
  direction,
  children: weights.map((weight) => ({ node: leaf(), weight })),
});

describe("resizePaneInDirection", () => {
  it("moves the focused pane's nearest boundary", () => {
    const root = split("horizontal", [1, 1, 1]);
    const left = resizePaneInDirection(root, "1", "left", 0.1) as LayoutSplit;
    expect(left.children.map((child) => child.weight)).toEqual([0.8, 1.2, 1]);

    const right = resizePaneInDirection(root, "1", "right", 0.1) as LayoutSplit;
    expect(right.children.map((child) => child.weight)).toEqual([1, 1.2, 0.8]);
  });

  it("walks outward through tabs to find the nearest matching boundary", () => {
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
      direction: "vertical",
      children: [
        { node: tabs, weight: 1 },
        { node: leaf(), weight: 1 },
      ],
    };
    const resized = resizePaneInDirection(
      root,
      "0.1",
      "down",
      0.1,
    ) as LayoutSplit;
    expect(resized.children.map((child) => child.weight)).toEqual([1.2, 0.8]);
  });

  it("does nothing at an outer edge or on the wrong axis", () => {
    const root = split("horizontal", [1, 1]);
    expect(resizePaneInDirection(root, "0", "left")).toBeNull();
    expect(resizePaneInDirection(root, "0", "up")).toBeNull();
  });

  it("balances tiled splits recursively without changing tabs", () => {
    const tabs = split("tabs", [2, 3]);
    const root: LayoutSplit = {
      type: "split",
      direction: "horizontal",
      children: [
        { node: tabs, weight: 2 },
        { node: split("vertical", [3, 4]), weight: 5 },
      ],
    };
    const balanced = balanceLayout(root) as LayoutSplit;
    expect(balanced.children.map((child) => child.weight)).toEqual([1, 1]);
    expect(
      (balanced.children[0].node as LayoutSplit).children.map(
        (child) => child.weight,
      ),
    ).toEqual([2, 3]);
    expect(
      (balanced.children[1].node as LayoutSplit).children.map(
        (child) => child.weight,
      ),
    ).toEqual([1, 1]);
  });
});
