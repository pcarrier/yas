import { describe, expect, it } from "vitest";
import {
  spatialNeighbor,
  type SpatialPaneRect,
} from "../layout/spatialNavigation";

const pane = (
  paneId: string,
  left: number,
  top: number,
  width = 100,
  height = 100,
): SpatialPaneRect => ({
  paneId,
  left,
  top,
  right: left + width,
  bottom: top + height,
});

describe("spatialNeighbor", () => {
  it("moves in all four visual directions", () => {
    const panes = [
      pane("center", 100, 100),
      pane("left", 0, 100),
      pane("right", 200, 100),
      pane("up", 100, 0),
      pane("down", 100, 200),
    ];
    expect(spatialNeighbor(panes, "center", "left")).toBe("left");
    expect(spatialNeighbor(panes, "center", "right")).toBe("right");
    expect(spatialNeighbor(panes, "center", "up")).toBe("up");
    expect(spatialNeighbor(panes, "center", "down")).toBe("down");
  });

  it("prefers overlap on the perpendicular axis over a closer diagonal", () => {
    const panes = [
      pane("current", 200, 100),
      pane("overlapping", 0, 120),
      pane("diagonal", 150, 220),
    ];
    expect(spatialNeighbor(panes, "current", "left")).toBe("overlapping");
  });

  it("chooses the nearest edge among overlapping candidates", () => {
    const panes = [
      pane("current", 300, 100),
      pane("far", 0, 100),
      pane("near", 180, 100),
    ];
    expect(spatialNeighbor(panes, "current", "left")).toBe("near");
  });

  it("uses recency only to break geometric ties", () => {
    const panes = [
      pane("current", 100, 100),
      pane("older", 0, 50, 100, 100),
      pane("recent", 0, 150, 100, 100),
    ];
    expect(spatialNeighbor(panes, "current", "left", ["recent", "older"])).toBe(
      "recent",
    );
  });

  it("returns null at an edge", () => {
    expect(spatialNeighbor([pane("only", 0, 0)], "only", "right")).toBeNull();
  });
});
