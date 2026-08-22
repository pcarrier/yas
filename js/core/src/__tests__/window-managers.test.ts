import { describe, expect, it } from "vitest";
import { parseDSL, serializeDSL } from "../layout/dsl";
import type { LayoutSplit } from "../layout/dsl";
import {
  cascadeRect,
  clampRect,
  enumeratePanes,
  nextWindowManager,
  toWindowManager,
  windowManagerOf,
  carryAssignmentsToPanes,
  SCROLLING_DEFAULT_WIDTH,
} from "../layout/tree";

describe("the DSL carries the window manager", () => {
  it("reads a scrolling strip", () => {
    const { root } = parseDSL("scroll(_ 0.5, col(_, _) 0.75, _)");
    expect(windowManagerOf(root)).toBe("scrolling");
    const split = root as LayoutSplit;
    expect(split.children.map((child) => child.weight)).toEqual([0.5, 0.75, 1]);
    // A column may be a stack, which is what makes Ctrl+B v meaningful in a
    // strip: it divides the column, not the strip.
    expect(split.children[1].node.type).toBe("split");
  });

  it("reads floating frames", () => {
    const { root } = parseDSL("float(_ [10,10,40,30], _ [55,20,40,50])");
    expect(windowManagerOf(root)).toBe("floating");
    const split = root as LayoutSplit;
    expect(split.children[0].rect).toEqual({
      x: 10,
      y: 10,
      width: 40,
      height: 30,
    });
    expect(split.children[1].rect?.x).toBe(55);
  });

  it("round-trips both through the serializer", () => {
    for (const dsl of [
      "scroll(_ 0.5, _ 0.5)",
      "scroll(_ 0.5, col(_, _) 1)",
      "float(_ [10,10,40,30], _ [55,20,40,50])",
      "float(_ [-4,0,40,30], _ [0,0,100,100])",
    ]) {
      const { root, weight } = parseDSL(dsl);
      expect(parseDSL(serializeDSL(root, weight)).root).toEqual(root);
    }
  });

  it("keeps a tiling tree on tiling", () => {
    expect(windowManagerOf(parseDSL("line(_, _)").root)).toBe("tiling");
    expect(windowManagerOf(parseDSL("tabs(_, _)").root)).toBe("tiling");
    expect(windowManagerOf(parseDSL("_").root)).toBe("tiling");
  });

  it("rejects a frame with no area or an absurd one", () => {
    expect(() => parseDSL("float(_ [0,0,0,30], _ [0,0,10,10])")).toThrow(
      "no area",
    );
    expect(() => parseDSL("float(_ [0,0,900,30], _ [0,0,10,10])")).toThrow(
      "out of range",
    );
  });
});

describe("toWindowManager", () => {
  const tiled = parseDSL("line(_ 2, col(_, _))").root;

  it("flattens a nested tree into a strip of columns", () => {
    const strip = toWindowManager(tiled, "scrolling") as LayoutSplit;
    expect(strip.direction).toBe("scrolling");
    expect(strip.children).toHaveLength(3);
    for (const child of strip.children) {
      expect(child.node.type).toBe("leaf");
      expect(child.weight).toBe(SCROLLING_DEFAULT_WIDTH);
    }
  });

  it("gives every floating window a frame, cascaded", () => {
    const floating = toWindowManager(tiled, "floating") as LayoutSplit;
    expect(floating.direction).toBe("floating");
    expect(floating.children.map((child) => child.rect)).toEqual([
      cascadeRect(0),
      cascadeRect(1),
      cascadeRect(2),
    ]);
  });

  it("leaves one window alone rather than splitting it against nothing", () => {
    expect(toWindowManager(parseDSL("_").root, "tiling").type).toBe("leaf");
  });

  it("preserves order, so occupants land back where they were", () => {
    const before = enumeratePanes(tiled);
    const strip = toWindowManager(tiled, "scrolling");
    const after = enumeratePanes(strip);
    const carried = carryAssignmentsToPanes({
      currentPanes: before,
      nextPanes: after,
      previous: {
        assignments: {
          [before[0].id]: "a",
          [before[1].id]: "b",
          [before[2].id]: "c",
        },
      },
      liveSessionIds: ["a", "b", "c"],
    });
    expect(after.map((pane) => carried.assignments[pane.id])).toEqual([
      "a",
      "b",
      "c",
    ]);
  });

  it("cycles tiling → scrolling → floating → tiling", () => {
    expect(nextWindowManager("tiling")).toBe("scrolling");
    expect(nextWindowManager("scrolling")).toBe("floating");
    expect(nextWindowManager("floating")).toBe("tiling");
  });
});

describe("clampRect", () => {
  it("keeps a sliver of a dragged-away window reachable", () => {
    expect(clampRect({ x: -400, y: 10, width: 40, height: 30 }).x).toBe(-34);
    expect(clampRect({ x: 400, y: 10, width: 40, height: 30 }).x).toBe(94);
    expect(clampRect({ x: 10, y: -50, width: 40, height: 30 }).y).toBe(0);
    expect(clampRect({ x: 10, y: 400, width: 40, height: 30 }).y).toBe(94);
  });

  it("refuses a window too small to grab or bigger than twice the viewport", () => {
    const tiny = clampRect({ x: 10, y: 10, width: 0.5, height: 0.5 });
    expect(tiny.width).toBe(8);
    expect(tiny.height).toBe(6);
    const huge = clampRect({ x: 0, y: 0, width: 5_000, height: 5_000 });
    expect(huge.width).toBe(200);
    expect(huge.height).toBe(200);
  });
});
