import { describe, expect, it } from "vitest";
import type { YasSession, YasSurface } from "@yas-run/core";
import { parseDSL } from "@yas-run/core/layout";
import {
  floatingLayerStackingStyle,
  floatingFrameNodes,
  newlyLaunchedSurface,
  floatingDropAppendsWindow,
  shouldOpenAsFloatingWindow,
  floatingPaneIds,
  floatingWindowTitle,
  addFloatingWindowToWorkspace,
  addTiledWindowToWorkspace,
  appendFloatingWindow,
  isFloatingPane,
  panesByFloatingMode,
  reusableFloatingPaneId,
  resizeFloatingRect,
  rebaseFloatingRect,
  snapFloatingRect,
  togglePaneFloating,
} from "../layout/floatingWindow";
import {
  mergeUniquePaneAssignments,
  uniquePaneValues,
} from "../layout/assignmentOwnership";

describe("floating window chrome", () => {
  it("only floats ordinary opens under the global floating manager", () => {
    expect(shouldOpenAsFloatingWindow(parseDSL("_").root)).toBe(false);
    expect(shouldOpenAsFloatingWindow(parseDSL("line(_, _)").root)).toBe(false);
    expect(shouldOpenAsFloatingWindow(parseDSL("float(_, _)").root)).toBe(true);
  });

  it("preserves explicit floating opens in a mixed workspace", () => {
    const mixed = togglePaneFloating(
      parseDSL("line(_, _)").root,
      { "0": "left", "1": "right" },
      "1",
      { x: 20, y: 10, width: 60, height: 70 },
    );
    expect(mixed).not.toBeNull();
    expect(shouldOpenAsFloatingWindow(mixed!.root)).toBe(false);
    expect(shouldOpenAsFloatingWindow(mixed!.root, true)).toBe(true);
  });

  it("matches app metadata that arrives after a new surface create", () => {
    const existing = new Set(["local:1"]);
    const surface = (surfaceId: bigint): YasSurface => ({
      connectionId: "local",
      surfaceId,
      parentId: 0n,
      title: "Alacritty",
      appId: "Alacritty",
      width: 800,
      height: 600,
      logicalWidth: 800,
      logicalHeight: 600,
    });
    const surfaces = [surface(1n), surface(2n)];

    expect(
      newlyLaunchedSurface(surfaces, "local", "Alacritty", existing)?.surfaceId,
    ).toBe(2n);
  });

  it("keeps frame keys stable across viewport rebases and sibling removal", () => {
    const root = parseDSL("float(_, _)").root;
    expect(root.type).toBe("split");
    if (root.type !== "split") return;
    const before = floatingFrameNodes(root.children);
    const rebased = root.children.map((child, index) => ({
      ...child,
      rect: { x: index * 10, y: index * 10, width: 50, height: 50 },
    }));

    const afterRebase = floatingFrameNodes(rebased);
    expect(afterRebase[0]).toBe(before[0]);
    expect(afterRebase[1]).toBe(before[1]);
    expect(floatingFrameNodes(rebased.slice(1))).toEqual([before[1]]);
  });

  it("drops duplicate restored owners before they can mount two windows", () => {
    const surface = "surface:local:7";
    expect(
      uniquePaneValues({ "0": surface, "1": surface }, ["0", "1"]),
    ).toEqual({ "0": surface });
    expect(
      mergeUniquePaneAssignments(
        { "0": null, "1": null },
        { "0": surface, "1": surface },
        ["0", "1"],
      ),
    ).toEqual({ "0": surface, "1": null });
  });

  it("isolates window stacking below portal-based panels", () => {
    expect(floatingLayerStackingStyle).toEqual({
      isolation: "isolate",
      "z-index": 0,
    });
  });

  it("addresses every leaf in a nested floating child", () => {
    const root = parseDSL("float(col(_,tabs(_,_)),_)").root;
    if (root.type !== "split") throw new Error("expected a floating split");
    expect(floatingPaneIds(root.children[0].node, [0])).toEqual([
      "0.0",
      "0.1.0",
      "0.1.1",
    ]);
    expect(floatingPaneIds(root.children[1].node, [1])).toEqual(["1"]);
  });

  it("appends a parked item as its own cascaded floating window", () => {
    const root = parseDSL("float(_ [6,6,58,58])").root;
    const appended = appendFloatingWindow(root);
    expect(appended?.paneId).toBe("1");
    expect(appended?.root.children).toHaveLength(2);
    expect(appended?.root.children[1].rect).toEqual({
      x: 10,
      y: 10,
      width: 58,
      height: 58,
    });
  });

  it("toggles one tiled view into the mixed floating scene and back", () => {
    const tiled = parseDSL("line(_, _)").root;
    const floated = togglePaneFloating(
      tiled,
      { "0": "left", "1": "right" },
      "1",
      { x: 20, y: 10, width: 60, height: 70 },
    );
    expect(floated).not.toBeNull();
    expect(floated?.root).toMatchObject({
      type: "split",
      direction: "workspace",
    });
    expect(floated?.assignments).toEqual({ "0": "left", "1": "right" });
    expect(isFloatingPane(floated!.root, "0")).toBe(false);
    expect(isFloatingPane(floated!.root, "1")).toBe(true);
    expect(panesByFloatingMode(floated!.root)).toEqual({
      tiled: ["0"],
      floating: ["1"],
    });

    const restored = togglePaneFloating(
      floated!.root,
      floated!.assignments,
      "1",
      { x: 0, y: 0, width: 1, height: 1 },
    );
    expect(restored?.root).toMatchObject({
      type: "split",
      direction: "horizontal",
    });
    expect(restored?.assignments).toEqual({ "0": "left", "1": "right" });
  });

  it("floats the only view without manufacturing an empty tiled pane", () => {
    const floated = togglePaneFloating(
      parseDSL("_").root,
      { "0": "only" },
      "0",
      { x: 14, y: 12, width: 72, height: 72 },
    );
    expect(floated?.root).toMatchObject({
      type: "split",
      direction: "workspace",
      children: [{ rect: { x: 14, y: 12, width: 72, height: 72 } }],
    });
    expect(isFloatingPane(floated!.root, "0")).toBe(true);
  });

  it("tiles the next normal window behind a float-only scene", () => {
    const floated = togglePaneFloating(
      parseDSL("_").root,
      { "0": "first" },
      "0",
      { x: 14, y: 12, width: 72, height: 72 },
    )!;
    const added = addTiledWindowToWorkspace(
      floated.root,
      floated.assignments,
      "second",
    );
    expect(added?.assignments).toEqual({ "0": "second", "1": "first" });
    expect(panesByFloatingMode(added!.root)).toEqual({
      tiled: ["0"],
      floating: ["1"],
    });
  });

  it("restores a parked view as a float above an ordinary tiled layout", () => {
    const root = parseDSL("line(_, _)").root;
    const added = addFloatingWindowToWorkspace(
      root,
      { "0": "left", "1": "right" },
      "parked",
      { x: 18, y: 12, width: 58, height: 62 },
    );
    expect(added?.assignments).toEqual({
      "0.0": "left",
      "0.1": "right",
      "1": "parked",
    });
    expect(panesByFloatingMode(added!.root)).toEqual({
      tiled: ["0.0", "0.1"],
      floating: ["1"],
    });
  });

  it("restores into an empty workspace without leaving an empty pane", () => {
    const added = addFloatingWindowToWorkspace(
      parseDSL("_").root,
      { "0": null },
      "parked",
      { x: 18, y: 12, width: 58, height: 62 },
    );
    expect(added?.assignments).toEqual({ "0": "parked" });
    expect(panesByFloatingMode(added!.root)).toEqual({
      tiled: [],
      floating: ["0"],
    });
  });

  it("pulls one window out of the global floating manager into tiling", () => {
    const root = parseDSL("float(_ [8,8,50,50], _ [24,20,55,60])").root;
    const mixed = togglePaneFloating(
      root,
      { "0": "first", "1": "second" },
      "1",
      { x: 0, y: 0, width: 1, height: 1 },
    );
    expect(mixed?.assignments).toEqual({ "0": "second", "1": "first" });
    expect(panesByFloatingMode(mixed!.root)).toEqual({
      tiled: ["0"],
      floating: ["1"],
    });
  });

  it("reuses a closed frame without renumbering live siblings", () => {
    const root = parseDSL(
      "float(_ [3,4,30,31], _ [33,8,40,42], _ [17,51,45,46])",
    ).root;
    expect(
      reusableFloatingPaneId(
        root,
        { "0": "first", "1": null, "2": "third" },
        new Set(),
      ),
    ).toBe("1");
    expect(
      reusableFloatingPaneId(
        root,
        { "0": "first", "1": null, "2": "third" },
        new Set(["1"]),
      ),
    ).toBeNull();
  });

  it("appends sidebar drops even when they land over an existing frame", () => {
    expect(floatingDropAppendsWindow(undefined)).toBe(true);
    expect(floatingDropAppendsWindow(null)).toBe(true);
    expect(floatingDropAppendsWindow("0")).toBe(false);
  });

  it("uses terminal and surface titles instead of anonymous pane numbers", () => {
    const session = {
      id: "session-1",
      connectionId: "local",
      ptyId: 7n,
      tag: "",
      title: "editor",
      usedRows: 10,
      command: "nvim",
      state: "active",
      exitStatus: null,
    } as YasSession;
    expect(floatingWindowTitle(session.id, [session], null)).toBe(
      "editor · nvim",
    );

    const surface = {
      connectionId: "local",
      surfaceId: 9n,
      parentId: 0n,
      title: "Brave",
      appId: "brave-browser",
      width: 1280,
      height: 720,
      logicalWidth: 1280,
      logicalHeight: 720,
    } as YasSurface;
    expect(floatingWindowTitle("surface:local:9", [], surface)).toBe("Brave");
  });

  it("snaps moved windows to each nearby viewport edge", () => {
    expect(
      snapFloatingRect(
        { x: 0.8, y: 41.5, width: 40, height: 58 },
        "move",
        1,
        1,
      ),
    ).toEqual({ x: 0, y: 42, width: 40, height: 58 });
    expect(
      snapFloatingRect(
        { x: 59.4, y: 0.6, width: 40, height: 40 },
        "move",
        1,
        1,
      ),
    ).toEqual({ x: 60, y: 0, width: 40, height: 40 });
  });

  it("snaps the resize handle to the right and bottom edges", () => {
    expect(
      snapFloatingRect({ x: 10, y: 10, width: 89.3, height: 89.4 }, "se", 1, 1),
    ).toEqual({ x: 10, y: 10, width: 90, height: 90 });
  });

  it("snaps moved and resized windows to neighboring edges", () => {
    const neighbor = { x: 50, y: 20, width: 30, height: 40 };
    expect(
      snapFloatingRect({ x: 9.4, y: 21, width: 40, height: 30 }, "move", 1, 1, [
        neighbor,
      ]),
    ).toEqual({ x: 10, y: 20, width: 40, height: 30 });
    expect(
      snapFloatingRect({ x: 10, y: 21, width: 39.3, height: 30 }, "e", 1, 1, [
        neighbor,
      ]),
    ).toEqual({ x: 10, y: 21, width: 40, height: 30 });
  });

  it("does not snap to a window outside the perpendicular range", () => {
    expect(
      snapFloatingRect({ x: 9.4, y: 70, width: 40, height: 20 }, "move", 1, 1, [
        { x: 50, y: 10, width: 30, height: 20 },
      ]),
    ).toEqual({ x: 9.4, y: 70, width: 40, height: 20 });
  });

  it("keeps opposite sides anchored for all resize directions", () => {
    const start = { x: 20, y: 20, width: 40, height: 30 };
    expect(resizeFloatingRect(start, -5, -4, "nw")).toEqual({
      x: 15,
      y: 16,
      width: 45,
      height: 34,
    });
    expect(resizeFloatingRect(start, 5, 4, "se")).toEqual({
      x: 20,
      y: 20,
      width: 45,
      height: 34,
    });
    expect(resizeFloatingRect(start, 5, 4, "ne")).toEqual({
      x: 20,
      y: 24,
      width: 45,
      height: 26,
    });
    expect(resizeFloatingRect(start, -5, 4, "sw")).toEqual({
      x: 15,
      y: 20,
      width: 45,
      height: 34,
    });
  });

  it("preserves the anchored side at the minimum size", () => {
    expect(
      resizeFloatingRect({ x: 20, y: 20, width: 40, height: 30 }, 100, 0, "w"),
    ).toEqual({ x: 52, y: 20, width: 8, height: 30 });
  });

  it("keeps a bottom-right window anchored when the sidebar changes width", () => {
    expect(
      rebaseFloatingRect(
        { x: 42, y: 42, width: 58, height: 58 },
        { left: 0, top: 0, width: 1000, height: 800 },
        { left: 0, top: 0, width: 800, height: 800 },
      ),
    ).toEqual({ x: 27.5, y: 42, width: 72.5, height: 58 });
  });

  it("preserves an unsnapped window's screen position and pixel size", () => {
    expect(
      rebaseFloatingRect(
        { x: 20, y: 25, width: 40, height: 50 },
        { left: 100, top: 50, width: 1000, height: 800 },
        { left: 100, top: 50, width: 800, height: 1000 },
      ),
    ).toEqual({ x: 25, y: 20, width: 50, height: 40 });
  });
});
