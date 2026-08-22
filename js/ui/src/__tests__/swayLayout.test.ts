import type { LayoutNode, WorkspaceLayout } from "@yas-run/core/layout";
import { describe, expect, it } from "vitest";
import {
  movePaneInDirection,
  setPaneLayout,
  splitPaneWithAssignment,
  togglePaneSplit,
} from "../layout/swayLayout";

describe("sway-like layout engine", () => {
  it("extends a matching split instead of nesting a BSP staircase", () => {
    const root = (
      {
        name: "Test layout",
        root: {
          type: "split",
          direction: "horizontal",
          children: [
            { node: { type: "leaf" }, weight: 1 },
            { node: { type: "leaf" }, weight: 1 },
          ],
        } as LayoutNode,
      } as WorkspaceLayout
    ).root;
    const result = splitPaneWithAssignment(
      root,
      { "0": "a", "1": "b" },
      "0",
      "c",
      "horizontal",
    );

    expect(result).not.toBeNull();
    expect(result!.root).toEqual({
      type: "split",
      direction: "horizontal",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    } as LayoutNode);
    expect(result!.assignments).toEqual({ "0": "a", "1": "c", "2": "b" });
    expect(result!.focusedPaneId).toBe("1");
    expect(result!.paneIdMap.get("1")).toBe("2");
  });

  it("nests when the requested split axis differs", () => {
    const root = (
      {
        name: "Test layout",
        root: {
          type: "split",
          direction: "horizontal",
          children: [
            { node: { type: "leaf" }, weight: 1 },
            { node: { type: "leaf" }, weight: 1 },
          ],
        } as LayoutNode,
      } as WorkspaceLayout
    ).root;
    const result = splitPaneWithAssignment(
      root,
      { "0": "a", "1": "b" },
      "0",
      "c",
      "vertical",
    );

    expect(result!.root).toEqual({
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
    } as LayoutNode);
    expect(result!.assignments).toEqual({ "0.0": "a", "0.1": "c", "1": "b" });
    expect(result!.focusedPaneId).toBe("0.1");
  });

  it("can insert a populated split on the leading edge", () => {
    const root = (
      {
        name: "Test layout",
        root: {
          type: "split",
          direction: "horizontal",
          children: [
            { node: { type: "leaf" }, weight: 1 },
            { node: { type: "leaf" }, weight: 1 },
          ],
        } as LayoutNode,
      } as WorkspaceLayout
    ).root;
    const result = splitPaneWithAssignment(
      root,
      { "0": "a", "1": "b" },
      "0",
      "c",
      "horizontal",
      false,
    );

    expect(result!.root).toEqual({
      type: "split",
      direction: "horizontal",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    } as LayoutNode);
    expect(result!.assignments).toEqual({ "0": "c", "1": "a", "2": "b" });
    expect(result!.focusedPaneId).toBe("0");
  });

  it("extends tabs only when the containing layout is already explicitly tabbed", () => {
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
    const result = splitPaneWithAssignment(
      root,
      { "0": "a", "1": "b" },
      "0",
      "c",
      "tabs",
    );

    expect(result!.root).toEqual({
      type: "split",
      direction: "tabs",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    } as LayoutNode);
    expect(result!.assignments).toEqual({ "0": "a", "1": "c", "2": "b" });
    expect(result!.focusedPaneId).toBe("1");
  });

  it("changes a container between split, tabbed, and stacking layouts", () => {
    const root = (
      {
        name: "Test layout",
        root: {
          type: "split",
          direction: "horizontal",
          children: [
            { node: { type: "leaf" }, weight: 1 },
            { node: { type: "leaf" }, weight: 1 },
          ],
        } as LayoutNode,
      } as WorkspaceLayout
    ).root;
    const tabs = setPaneLayout(root, { "0": "a", "1": "b" }, "1", "tabs");
    expect(tabs!.root).toEqual({
      type: "split",
      direction: "tabs",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    } as LayoutNode);
    expect(tabs!.assignments).toEqual({ "0": "a", "1": "b" });

    const stack = setPaneLayout(tabs!.root, tabs!.assignments, "1", "stacking");
    expect(stack!.root).toEqual({
      type: "split",
      direction: "stacking",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    } as LayoutNode);

    const horizontal = togglePaneSplit(stack!.root, stack!.assignments, "1");
    expect(horizontal!.root).toEqual({
      type: "split",
      direction: "horizontal",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    } as LayoutNode);
  });

  it("swaps adjacent siblings in the requested direction", () => {
    const root = (
      {
        name: "Test layout",
        root: {
          type: "split",
          direction: "horizontal",
          children: [
            { node: { type: "leaf" }, weight: 1 },
            { node: { type: "leaf" }, weight: 1 },
            { node: { type: "leaf" }, weight: 1 },
          ],
        } as LayoutNode,
      } as WorkspaceLayout
    ).root;
    const result = movePaneInDirection(
      root,
      { "0": "a", "1": "b", "2": "c" },
      "1",
      "0",
      "left",
    );

    expect(result!.root).toEqual({
      type: "split",
      direction: "horizontal",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    } as LayoutNode);
    expect(result!.assignments).toEqual({ "0": "b", "1": "a", "2": "c" });
    expect(result!.focusedPaneId).toBe("0");
  });

  it("moves across nested containers without turning the target into tabs", () => {
    const root = (
      {
        name: "Test layout",
        root: {
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
        } as LayoutNode,
      } as WorkspaceLayout
    ).root;
    const result = movePaneInDirection(
      root,
      { "0.0": "a", "0.1": "b", "1": "c" },
      "0.1",
      "1",
      "right",
    );

    expect(result!.root).toEqual({
      type: "split",
      direction: "horizontal",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    } as LayoutNode);
    expect(result!.assignments).toEqual({ "0": "a", "1": "c", "2": "b" });
    expect(result!.focusedPaneId).toBe("2");
  });

  it("moves a container to a new root edge when there is no neighbor", () => {
    const root = (
      {
        name: "Test layout",
        root: {
          type: "split",
          direction: "vertical",
          children: [
            { node: { type: "leaf" }, weight: 1 },
            { node: { type: "leaf" }, weight: 1 },
          ],
        } as LayoutNode,
      } as WorkspaceLayout
    ).root;
    const result = movePaneInDirection(
      root,
      { "0": "a", "1": "b" },
      "1",
      null,
      "right",
    );

    expect(result!.root).toEqual({
      type: "split",
      direction: "horizontal",
      children: [
        { node: { type: "leaf" }, weight: 1 },
        { node: { type: "leaf" }, weight: 1 },
      ],
    } as LayoutNode);
    expect(result!.assignments).toEqual({ "0": "a", "1": "b" });
    expect(result!.focusedPaneId).toBe("1");
  });

  it("pulls the active tab into a split when dropped on its own edge", () => {
    const root = (
      {
        name: "Test layout",
        root: {
          type: "split",
          direction: "horizontal",
          children: [
            {
              node: {
                type: "split",
                direction: "tabs",
                children: [
                  { node: { type: "leaf" }, weight: 1 },
                  { node: { type: "leaf" }, weight: 1 },
                ],
              },
              weight: 1,
            },
            { node: { type: "leaf" }, weight: 1 },
          ],
        } as LayoutNode,
      } as WorkspaceLayout
    ).root;
    const result = movePaneInDirection(
      root,
      { "0.0": "a", "0.1": "b", "1": "c" },
      "0.0",
      "0.0",
      "down",
    );

    expect(result!.root).toEqual({
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
    } as LayoutNode);
    expect(result!.assignments).toEqual({ "0.0": "b", "0.1": "a", "1": "c" });
    expect(result!.focusedPaneId).toBe("0.1");
  });
});
