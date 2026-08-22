import { describe, expect, it } from "vitest";
import { layoutFromDSL, serializeDSL } from "@yas-run/core/layout";
import {
  movePaneInDirection,
  setPaneLayout,
  splitPaneWithAssignment,
  togglePaneSplit,
} from "../layout/swayLayout";

describe("sway-like layout engine", () => {
  it("extends a matching split instead of nesting a BSP staircase", () => {
    const root = layoutFromDSL("line(_, _)").root;
    const result = splitPaneWithAssignment(
      root,
      { "0": "a", "1": "b" },
      "0",
      "c",
      "horizontal",
    );

    expect(result).not.toBeNull();
    expect(serializeDSL(result!.root)).toBe("line(_, _, _)");
    expect(result!.assignments).toEqual({ "0": "a", "1": "c", "2": "b" });
    expect(result!.focusedPaneId).toBe("1");
    expect(result!.paneIdMap.get("1")).toBe("2");
  });

  it("nests when the requested split axis differs", () => {
    const root = layoutFromDSL("line(_, _)").root;
    const result = splitPaneWithAssignment(
      root,
      { "0": "a", "1": "b" },
      "0",
      "c",
      "vertical",
    );

    expect(serializeDSL(result!.root)).toBe("line(col(_, _), _)");
    expect(result!.assignments).toEqual({ "0.0": "a", "0.1": "c", "1": "b" });
    expect(result!.focusedPaneId).toBe("0.1");
  });

  it("can insert a populated split on the leading edge", () => {
    const root = layoutFromDSL("line(_, _)").root;
    const result = splitPaneWithAssignment(
      root,
      { "0": "a", "1": "b" },
      "0",
      "c",
      "horizontal",
      false,
    );

    expect(serializeDSL(result!.root)).toBe("line(_, _, _)");
    expect(result!.assignments).toEqual({ "0": "c", "1": "a", "2": "b" });
    expect(result!.focusedPaneId).toBe("0");
  });

  it("extends tabs only when the containing layout is already explicitly tabbed", () => {
    const root = layoutFromDSL("tabs(_, _)").root;
    const result = splitPaneWithAssignment(
      root,
      { "0": "a", "1": "b" },
      "0",
      "c",
      "tabs",
    );

    expect(serializeDSL(result!.root)).toBe("tabs(_, _, _)");
    expect(result!.assignments).toEqual({ "0": "a", "1": "c", "2": "b" });
    expect(result!.focusedPaneId).toBe("1");
  });

  it("changes a container between split, tabbed, and stacking layouts", () => {
    const root = layoutFromDSL("line(_, _)").root;
    const tabs = setPaneLayout(root, { "0": "a", "1": "b" }, "1", "tabs");
    expect(serializeDSL(tabs!.root)).toBe("tabs(_, _)");
    expect(tabs!.assignments).toEqual({ "0": "a", "1": "b" });

    const stack = setPaneLayout(tabs!.root, tabs!.assignments, "1", "stacking");
    expect(serializeDSL(stack!.root)).toBe("stack(_, _)");

    const horizontal = togglePaneSplit(stack!.root, stack!.assignments, "1");
    expect(serializeDSL(horizontal!.root)).toBe("line(_, _)");
  });

  it("swaps adjacent siblings in the requested direction", () => {
    const root = layoutFromDSL("line(_, _, _)").root;
    const result = movePaneInDirection(
      root,
      { "0": "a", "1": "b", "2": "c" },
      "1",
      "0",
      "left",
    );

    expect(serializeDSL(result!.root)).toBe("line(_, _, _)");
    expect(result!.assignments).toEqual({ "0": "b", "1": "a", "2": "c" });
    expect(result!.focusedPaneId).toBe("0");
  });

  it("moves across nested containers without turning the target into tabs", () => {
    const root = layoutFromDSL("line(col(_, _), _)").root;
    const result = movePaneInDirection(
      root,
      { "0.0": "a", "0.1": "b", "1": "c" },
      "0.1",
      "1",
      "right",
    );

    expect(serializeDSL(result!.root)).toBe("line(_, _, _)");
    expect(result!.assignments).toEqual({ "0": "a", "1": "c", "2": "b" });
    expect(result!.focusedPaneId).toBe("2");
  });

  it("moves a container to a new root edge when there is no neighbor", () => {
    const root = layoutFromDSL("col(_, _)").root;
    const result = movePaneInDirection(
      root,
      { "0": "a", "1": "b" },
      "1",
      null,
      "right",
    );

    expect(serializeDSL(result!.root)).toBe("line(_, _)");
    expect(result!.assignments).toEqual({ "0": "a", "1": "b" });
    expect(result!.focusedPaneId).toBe("1");
  });

  it("pulls the active tab into a split when dropped on its own edge", () => {
    const root = layoutFromDSL("line(tabs(_, _), _)").root;
    const result = movePaneInDirection(
      root,
      { "0.0": "a", "0.1": "b", "1": "c" },
      "0.0",
      "0.0",
      "down",
    );

    expect(serializeDSL(result!.root)).toBe("line(col(_, _), _)");
    expect(result!.assignments).toEqual({ "0.0": "b", "0.1": "a", "1": "c" });
    expect(result!.focusedPaneId).toBe("0.1");
  });
});
