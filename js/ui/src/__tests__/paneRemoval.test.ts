import type { LayoutNode } from "@yas-run/core/layout";
import { describe, expect, it } from "vitest";
import { enumeratePanes } from "@yas-run/core/layout";
import {
  pruneUnassignedPanes,
  removePaneFromLayout,
  showEmptyPaneHint,
} from "../layout/paneRemoval";

describe("showEmptyPaneHint", () => {
  it("shows one focused hint when a multi-pane manager has no occupants", () => {
    expect(showEmptyPaneHint(true, false, true)).toBe(true);
    expect(showEmptyPaneHint(true, false, false)).toBe(false);
    expect(showEmptyPaneHint(true, true, true)).toBe(false);
    expect(showEmptyPaneHint(false, true, false)).toBe(true);
  });
});

describe("removePaneFromLayout", () => {
  it("removes a floating window instead of leaving an empty shell", () => {
    const root = {
      type: "split",
      direction: "workspace",
      children: [
        {
          node: { type: "leaf" },
          weight: 1,
          rect: { x: 3, y: 4, width: 30, height: 31 },
        },
        {
          node: { type: "leaf" },
          weight: 1,
          rect: { x: 33, y: 8, width: 40, height: 42 },
        },
        {
          node: { type: "leaf" },
          weight: 1,
          rect: { x: 17, y: 51, width: 45, height: 46 },
        },
      ],
    } as LayoutNode;
    if (root.type !== "split") throw new Error("expected floating root");
    const first = root.children[0];
    const third = root.children[2];
    const next = removePaneFromLayout(root, "1");

    expect(next).not.toBeNull();
    expect(next!).toEqual({
      type: "split",
      direction: "workspace",
      children: [
        {
          node: { type: "leaf" },
          weight: 1,
          rect: { x: 3, y: 4, width: 30, height: 31 },
        },
        {
          node: { type: "leaf" },
          weight: 1,
          rect: { x: 17, y: 51, width: 45, height: 46 },
        },
      ],
    } as LayoutNode);
    expect(enumeratePanes(next!).map((pane) => pane.id)).toEqual(["0", "1"]);
    if (next!.type !== "split") throw new Error("expected floating root");
    // Solid's keyed <For> can now retain both live frames. Exact child
    // identity matters here: replacing the later object remounts its terminal
    // or surface even when its serialized rectangle is unchanged.
    expect(next!.children[0]).toBe(first);
    expect(next!.children[1]).toBe(third);
  });

  it("keeps the floating manager around its sole surviving window", () => {
    const root = {
      type: "split",
      direction: "workspace",
      children: [
        {
          node: { type: "leaf" },
          weight: 1,
          rect: { x: 6, y: 6, width: 58, height: 58 },
        },
        {
          node: { type: "leaf" },
          weight: 1,
          rect: { x: 12, y: 12, width: 50, height: 50 },
        },
      ],
    } as LayoutNode;
    const next = removePaneFromLayout(root, "0");

    expect(next).not.toBeNull();
    expect(next!).toEqual({
      type: "split",
      direction: "workspace",
      children: [
        {
          node: { type: "leaf" },
          weight: 1,
          rect: { x: 12, y: 12, width: 50, height: 50 },
        },
      ],
    } as LayoutNode);
    expect(enumeratePanes(next!).map((pane) => pane.id)).toEqual(["0"]);
  });

  it("collapses a mixed scene after its last floating window is removed", () => {
    const root = {
      type: "split",
      direction: "workspace",
      children: [
        {
          node: {
            type: "split",
            direction: "horizontal",
            children: [
              { node: { type: "leaf" }, weight: 1 },
              { node: { type: "leaf" }, weight: 1 },
            ],
          },
          weight: 1,
        },
        {
          node: { type: "leaf" },
          weight: 1,
          rect: { x: 12, y: 12, width: 50, height: 50 },
        },
      ],
    } as LayoutNode;
    const next = removePaneFromLayout(root, "1");
    expect(next).not.toBeNull();
    expect(next!).toEqual({
      type: "split",
      direction: "horizontal",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    } as LayoutNode);
  });

  it("keeps a mixed scene when only its floating window survives", () => {
    const root = {
      type: "split",
      direction: "workspace",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        {
          node: { type: "leaf" },
          weight: 1,
          rect: { x: 12, y: 12, width: 50, height: 50 },
        },
      ],
    } as LayoutNode;
    const next = removePaneFromLayout(root, "0");
    expect(next).not.toBeNull();
    expect(next!).toEqual({
      type: "split",
      direction: "workspace",
      children: [
        {
          node: { type: "leaf" },
          weight: 1,
          rect: { x: 12, y: 12, width: 50, height: 50 },
        },
      ],
    } as LayoutNode);
  });

  it("collapses a singleton nested split", () => {
    const root = {
      type: "split",
      direction: "horizontal",
      children: [
        {
          node: {
            type: "split",
            direction: "vertical",
            children: [
              { node: { type: "leaf" }, weight: 1 },
              { node: { type: "leaf" }, weight: 1 },
            ],
          },
          weight: 1,
        },
        { node: { type: "leaf" }, weight: 1 },
      ],
    } as LayoutNode;
    const next = removePaneFromLayout(root, "0.1");

    expect(next).not.toBeNull();
    expect(next!).toEqual({
      type: "split",
      direction: "horizontal",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    } as LayoutNode);
  });

  it("returns the sole surviving leaf for a two-pane layout", () => {
    const root = {
      type: "split",
      direction: "horizontal",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    } as LayoutNode;
    const next = removePaneFromLayout(root, "0");

    expect(next).toEqual({ type: "leaf" });
  });

  it("does not change the tree for an unknown pane id", () => {
    const root = {
      type: "split",
      direction: "horizontal",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    } as LayoutNode;
    expect(removePaneFromLayout(root, "9")).toBe(root);
  });
});

describe("pruneUnassignedPanes", () => {
  it("removes every empty branch and rekeys surviving assignments", () => {
    const root = {
      type: "split",
      direction: "horizontal",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        {
          node: {
            type: "split",
            direction: "vertical",
            children: [
              { node: { type: "leaf" }, weight: 1 },
              { node: { type: "leaf" }, weight: 1 },
            ],
          },
          weight: 1,
        },
        { node: { type: "leaf" }, weight: 1 },
      ],
    } as LayoutNode;
    const result = pruneUnassignedPanes(root, {
      "0": null,
      "1.0": "terminal:a",
      "1.1": null,
      "2": "surface:local:3",
    });

    expect(result).not.toBeNull();
    expect(result!.root).toEqual({
      type: "split",
      direction: "horizontal",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    } as LayoutNode);
    expect(result!.assignments).toEqual({
      "0": "terminal:a",
      "1": "surface:local:3",
    });
    expect(result!.paneIdMap.get("1.0")).toBe("0");
    expect(result!.paneIdMap.get("2")).toBe("1");
  });

  it("retains only one launcher leaf when the workspace is empty", () => {
    const root = {
      type: "split",
      direction: "tabs",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    } as LayoutNode;
    const result = pruneUnassignedPanes(root, {
      "0": null,
      "1": null,
      "2": null,
    });

    expect(result).not.toBeNull();
    expect(result!.root).toEqual({ type: "leaf" } as LayoutNode);
    expect(result!.assignments).toEqual({ "0": null });
  });

  it("does not create churn when no empty panes exist", () => {
    const root = {
      type: "split",
      direction: "horizontal",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    } as LayoutNode;
    expect(
      pruneUnassignedPanes(root, { "0": "terminal:a", "1": "terminal:b" }),
    ).toBeNull();
  });
});
