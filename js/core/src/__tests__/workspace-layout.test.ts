import { describe, expect, it } from "vitest";
import {
  LAYOUT_MAX_BYTES,
  LAYOUT_MAX_DEPTH,
  LAYOUT_MAX_PANES,
  leafCount,
  sameLayoutTree,
  validateLayoutNode,
  validateWorkspaceLayout,
  type LayoutNode,
  type LayoutSplit,
} from "../layout/model";
import { clampRect } from "../layout/tree";

const workspace: LayoutSplit = {
  type: "split",
  direction: "workspace",
  children: [
    {
      weight: 1,
      node: {
        type: "split",
        direction: "horizontal",
        children: [
          {
            node: { type: "leaf", command: "htop", fontSize: "120%" },
            weight: 2,
          },
          { node: { type: "leaf", fontSize: 14 }, weight: 1, label: "Editor" },
        ],
      },
    },
    {
      node: { type: "leaf" },
      weight: 1,
      rect: { x: 10, y: 12, width: 50, height: 60 },
    },
  ],
};

describe("structured workspace layouts", () => {
  it("round-trips a tree with floating windows and pane settings through JSON", () => {
    const layout = { name: "Development", root: workspace };
    const restored = validateWorkspaceLayout(
      JSON.parse(JSON.stringify(layout)),
    );
    expect(restored).toEqual(layout);
    expect(restored.root).not.toBe(workspace);
    expect(leafCount(restored.root)).toBe(3);
    expect(sameLayoutTree(restored.root, workspace)).toBe(true);
  });

  it.each(["horizontal", "vertical", "tabs", "stacking"])(
    "accepts %s containers",
    (direction) => {
      expect(
        validateLayoutNode({
          type: "split",
          direction,
          children: [
            { node: { type: "leaf" }, weight: 1 },
            { node: { type: "leaf" }, weight: 1 },
          ],
        }),
      ).toMatchObject({ direction });
    },
  );

  it("keeps the frame of a workspace containing only one floating window", () => {
    const root = { ...workspace, children: [workspace.children[1]] };
    expect(validateLayoutNode(root)).toEqual(root);
  });

  it.each([
    "line(_, _)",
    { name: "Old", dsl: "line(_, _)" },
    {
      name: "Old",
      root: { type: "split", direction: "scrolling", children: [] },
    },
    {
      name: "Old",
      root: { type: "split", direction: "floating", children: [] },
    },
  ])("rejects old text and mode representations without migration", (value) => {
    expect(() => validateWorkspaceLayout(value)).toThrow();
  });

  it.each([
    { type: "leaf", children: [] },
    { type: "leaf", command: 4 },
    { type: "leaf", fontSize: 0 },
    { type: "leaf", fontSize: "12em" },
    {
      type: "split",
      direction: "horizontal",
      children: [{ node: { type: "leaf" }, weight: 1 }],
    },
    { type: "split", direction: "workspace", children: [] },
    {
      type: "split",
      direction: "workspace",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    },
  ])("rejects malformed layout objects", (value) => {
    expect(() => validateLayoutNode(value)).toThrow();
  });

  it.each([0, -1, NaN, Infinity])("rejects invalid weights: %s", (weight) => {
    expect(() =>
      validateLayoutNode({
        ...workspace,
        children: [{ ...workspace.children[1], weight }],
      }),
    ).toThrow();
  });

  it.each([
    { x: 0, y: 0, width: 0, height: 30 },
    { x: 0, y: 0, width: 900, height: 30 },
    { x: NaN, y: 0, width: 50, height: 30 },
  ])("rejects invalid floating frames", (rect) => {
    expect(() =>
      validateLayoutNode({
        ...workspace,
        children: [{ ...workspace.children[1], rect }],
      }),
    ).toThrow();
  });

  it("bounds pane counts and depth before admitting a tree", () => {
    const wide: LayoutSplit = {
      type: "split",
      direction: "horizontal",
      children: Array.from({ length: LAYOUT_MAX_PANES }, () => ({
        node: { type: "leaf" },
        weight: 1,
      })),
    };
    expect(leafCount(validateLayoutNode(wide))).toBe(LAYOUT_MAX_PANES);
    expect(() =>
      validateLayoutNode({
        ...wide,
        children: [...wide.children, wide.children[0]],
      }),
    ).toThrow();
    let deep: LayoutNode = { type: "leaf" };
    for (let index = 0; index < LAYOUT_MAX_DEPTH; index++)
      deep = {
        type: "split",
        direction: "horizontal",
        children: [
          { node: { type: "leaf" }, weight: 1 },
          { node: deep, weight: 1 },
        ],
      };
    expect(() => validateLayoutNode(deep)).not.toThrow();
    expect(() =>
      validateLayoutNode({
        type: "split",
        direction: "horizontal",
        children: [
          { node: { type: "leaf" }, weight: 1 },
          { node: deep, weight: 1 },
        ],
      }),
    ).toThrow();
  });

  it("bounds total bytes across otherwise valid fields", () => {
    const label = "x".repeat(LAYOUT_MAX_BYTES / 2);
    expect(() =>
      validateLayoutNode({
        type: "split",
        direction: "tabs",
        children: [
          { node: { type: "leaf" }, weight: 1, label },
          { node: { type: "leaf" }, weight: 1, label },
        ],
      }),
    ).toThrow("byte limit");
  });

  it("compares tree contents rather than object key order", () => {
    expect(
      sameLayoutTree(
        { type: "leaf", command: "htop" },
        { command: "htop", type: "leaf" },
      ),
    ).toBe(true);
    const resized = structuredClone(workspace);
    resized.children[1].rect!.width += 1;
    expect(sameLayoutTree(workspace, resized)).toBe(false);
    expect(
      sameLayoutTree(
        { type: "leaf", fontSize: 14 },
        { type: "leaf", fontSize: 16 },
      ),
    ).toBe(false);
  });
});

describe("clampRect", () => {
  it("keeps a sliver of a dragged-away window reachable", () => {
    expect(clampRect({ x: -400, y: 10, width: 40, height: 30 }).x).toBe(-34);
    expect(clampRect({ x: 400, y: 10, width: 40, height: 30 }).x).toBe(94);
    expect(clampRect({ x: 10, y: -50, width: 40, height: 30 }).y).toBe(0);
    expect(clampRect({ x: 10, y: 400, width: 40, height: 30 }).y).toBe(94);
  });

  it("bounds window dimensions", () => {
    expect(clampRect({ x: 0, y: 0, width: 0.5, height: 0.5 })).toMatchObject({
      width: 8,
      height: 6,
    });
    expect(clampRect({ x: 0, y: 0, width: 5000, height: 5000 })).toMatchObject({
      width: 200,
      height: 200,
    });
  });
});
