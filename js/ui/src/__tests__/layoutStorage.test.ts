import { afterEach, describe, expect, it } from "vitest";
import type { WorkspaceLayout } from "@yas-run/core/layout";
import {
  LAYOUT_HISTORY_KEY,
  loadActiveLayoutState,
  loadRecentLayouts,
  removeFromHistory,
  saveActiveLayout,
  saveActiveLayoutState,
  saveToHistory,
} from "../layout/store";

const layout: WorkspaceLayout = {
  name: "Development",
  root: {
    type: "split",
    direction: "horizontal",
    children: [
      { node: { type: "leaf" }, weight: 2 },
      { node: { type: "leaf" }, weight: 1 },
    ],
  },
};

afterEach(() => {
  localStorage.removeItem("yas.layout");
  localStorage.removeItem(LAYOUT_HISTORY_KEY);
});

describe("layout tree storage", () => {
  it("persists the tree and retains assignments across an equivalent tree write", () => {
    saveActiveLayoutState(
      layout,
      { "0": "terminal:local:11", "1": "surface:local:12" },
      "1",
    );
    const saved = JSON.parse(localStorage.getItem("yas.layout")!);
    expect(saved).toEqual({
      ...layout,
      assignments: { "0": "terminal:local:11", "1": "surface:local:12" },
      focusedPaneId: "1",
    });
    saveActiveLayout(structuredClone(layout));
    expect(loadActiveLayoutState()).toEqual({
      layout,
      assignments: saved.assignments,
      focusedPaneId: "1",
    });
  });

  it("does not carry old pane paths into a different tree", () => {
    saveActiveLayoutState(layout, { "1": "terminal:local:11" }, "1");
    const next: WorkspaceLayout = { name: "Single", root: { type: "leaf" } };
    saveActiveLayout(next);
    expect(loadActiveLayoutState()).toEqual({
      layout: next,
      assignments: {},
      focusedPaneId: null,
    });
  });

  it("deduplicates recent layouts by tree and removes equivalent copies", () => {
    saveToHistory(layout);
    const renamed = { ...structuredClone(layout), name: "Renamed" };
    saveToHistory(renamed);
    expect(loadRecentLayouts()).toEqual([renamed]);
    removeFromHistory(structuredClone(layout));
    expect(loadRecentLayouts()).toEqual([]);
  });

  it("ignores old text layouts instead of converting them", () => {
    const old = { name: "Old", dsl: "line(_, _)" };
    localStorage.setItem("yas.layout", JSON.stringify(old));
    localStorage.setItem(LAYOUT_HISTORY_KEY, JSON.stringify([old, layout]));
    expect(loadActiveLayoutState()).toBeNull();
    expect(loadRecentLayouts()).toEqual([layout]);
  });

  it("rejects malformed saved trees", () => {
    localStorage.setItem(
      "yas.layout",
      JSON.stringify({
        ...layout,
        root: { type: "split", direction: "horizontal", children: [] },
      }),
    );
    expect(loadActiveLayoutState()).toBeNull();
  });
});
