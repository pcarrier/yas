import type { YasSession, YasSurface } from "@yas-run/core";
import {
  cascadeRect,
  clampRect,
  type LayoutChild,
  type LayoutNode,
  type LayoutRect,
  type LayoutSplit,
} from "@yas-run/core/layout";
import { tileDisplay } from "../ide/tileDisplay";
import { sessionName, surfaceName } from "../theme";
import { isTileAssignment, isWebAssignment } from "./store";

export type FloatingResizeEdge =
  | "n"
  | "ne"
  | "e"
  | "se"
  | "s"
  | "sw"
  | "w"
  | "nw";
export type FloatingDragMode = "move" | FloatingResizeEdge;
export interface FloatingViewport {
  left: number;
  top: number;
  width: number;
  height: number;
}

/**
 * Keep per-window stacking inside the workspace. Portal chrome such as the
 * session manager must remain above even the focused floating window.
 */
export const floatingLayerStackingStyle = {
  isolation: "isolate",
  "z-index": 0,
} as const;

/** Stable keys for floating frames across rect-wrapper rewrites. */
export function floatingFrameNodes(
  children: readonly LayoutChild[],
): LayoutNode[] {
  return children.map((child) => child.node);
}

/** Match a launch against identities that did not exist before its request.
 * Surface CREATE and app-id metadata are separate events, so the baseline is
 * launch-time state rather than the immediately preceding UI snapshot. */
export function newlyLaunchedSurface(
  surfaces: readonly YasSurface[],
  connectionId: string,
  appId: string,
  existingSurfaceKeys: ReadonlySet<string>,
): YasSurface | undefined {
  return surfaces.find(
    (surface) =>
      surface.connectionId === connectionId &&
      surface.appId === appId &&
      !existingSurfaceKeys.has(
        `${surface.connectionId}:${surface.surfaceId}`,
      ),
  );
}

/** Parked/sidebar content appends a window; pane-originated content moves. */
export function floatingDropAppendsWindow(
  sourcePaneId: string | null | undefined,
): boolean {
  return sourcePaneId == null;
}

/** Leaf pane ids below one managed window, in display order. */
export function floatingPaneIds(
  node: LayoutNode,
  path: readonly number[],
): string[] {
  if (node.type === "leaf") return [path.length > 0 ? path.join(".") : "0"];
  return node.children.flatMap((child, index) =>
    floatingPaneIds(child.node, [...path, index]),
  );
}

/** Append one independent window to a floating root. */
export function appendFloatingWindow(
  root: LayoutNode,
): { root: LayoutSplit; paneId: string } | null {
  if (root.type !== "split" || root.direction !== "floating") return null;
  const index = root.children.length;
  return {
    root: {
      ...root,
      children: [
        ...root.children,
        {
          node: { type: "leaf" },
          weight: 1,
          rect: cascadeRect(index),
        },
      ],
    },
    paneId: String(index),
  };
}

/** Reuse an invisible top-level frame without changing any sibling pane id. */
export function reusableFloatingPaneId(
  root: LayoutNode,
  assignments: Readonly<Record<string, string | null | undefined>>,
  reservedPaneIds: ReadonlySet<string>,
): string | null {
  if (root.type !== "split" || root.direction !== "floating") return null;
  const index = root.children.findIndex((child, at) => {
    const paneId = String(at);
    return (
      child.node.type === "leaf" &&
      assignments[paneId] == null &&
      !reservedPaneIds.has(paneId)
    );
  });
  return index < 0 ? null : String(index);
}

function rangesMeet(
  firstStart: number,
  firstEnd: number,
  secondStart: number,
  secondEnd: number,
  threshold: number,
): boolean {
  return (
    Math.min(firstEnd, secondEnd) >=
    Math.max(firstStart, secondStart) - threshold
  );
}

function nearestDelta(
  deltas: readonly number[],
  threshold: number,
): number | null {
  let best: number | null = null;
  for (const delta of deltas) {
    if (Math.abs(delta) > threshold) continue;
    if (best === null || Math.abs(delta) < Math.abs(best)) best = delta;
  }
  return best;
}

/** Magnetize a moved/resized frame to viewport and neighboring window edges. */
export function snapFloatingRect(
  rect: LayoutRect,
  mode: FloatingDragMode,
  thresholdX: number,
  thresholdY: number,
  neighbors: readonly LayoutRect[] = [],
): LayoutRect {
  const next = clampRect(rect);
  const horizontalTargets = (requireVerticalOverlap: boolean) => {
    const targets = [0, 100];
    for (const raw of neighbors) {
      const neighbor = clampRect(raw);
      if (
        requireVerticalOverlap &&
        !rangesMeet(
          next.y,
          next.y + next.height,
          neighbor.y,
          neighbor.y + neighbor.height,
          thresholdY,
        )
      )
        continue;
      targets.push(neighbor.x, neighbor.x + neighbor.width);
    }
    return targets;
  };
  const verticalTargets = (requireHorizontalOverlap: boolean) => {
    const targets = [0, 100];
    for (const raw of neighbors) {
      const neighbor = clampRect(raw);
      if (
        requireHorizontalOverlap &&
        !rangesMeet(
          next.x,
          next.x + next.width,
          neighbor.x,
          neighbor.x + neighbor.width,
          thresholdX,
        )
      )
        continue;
      targets.push(neighbor.y, neighbor.y + neighbor.height);
    }
    return targets;
  };
  if (mode === "move") {
    const xTargets = horizontalTargets(true);
    const xDelta = nearestDelta(
      xTargets.flatMap((target) => [
        target - next.x,
        target - (next.x + next.width),
      ]),
      thresholdX,
    );
    if (xDelta !== null) next.x += xDelta;

    const yTargets = verticalTargets(true);
    const yDelta = nearestDelta(
      yTargets.flatMap((target) => [
        target - next.y,
        target - (next.y + next.height),
      ]),
      thresholdY,
    );
    if (yDelta !== null) next.y += yDelta;
  } else {
    if (mode.includes("w")) {
      const delta = nearestDelta(
        horizontalTargets(true).map((target) => target - next.x),
        thresholdX,
      );
      if (delta !== null) {
        next.x += delta;
        next.width -= delta;
      }
    }
    if (mode.includes("e")) {
      const delta = nearestDelta(
        horizontalTargets(true).map(
          (target) => target - (next.x + next.width),
        ),
        thresholdX,
      );
      if (delta !== null) next.width += delta;
    }
    if (mode.includes("n")) {
      const delta = nearestDelta(
        verticalTargets(true).map((target) => target - next.y),
        thresholdY,
      );
      if (delta !== null) {
        next.y += delta;
        next.height -= delta;
      }
    }
    if (mode.includes("s")) {
      const delta = nearestDelta(
        verticalTargets(true).map(
          (target) => target - (next.y + next.height),
        ),
        thresholdY,
      );
      if (delta !== null) next.height += delta;
    }
  }
  const snapped = clampRect(next);
  const clean = (value: number) => Math.round(value * 1_000_000) / 1_000_000;
  return {
    x: clean(snapped.x),
    y: clean(snapped.y),
    width: clean(snapped.width),
    height: clean(snapped.height),
  };
}

/** Resize from one edge/corner while keeping its opposite sides anchored. */
export function resizeFloatingRect(
  start: LayoutRect,
  dx: number,
  dy: number,
  edge: FloatingResizeEdge,
): LayoutRect {
  let left = start.x;
  let right = start.x + start.width;
  let top = start.y;
  let bottom = start.y + start.height;
  if (edge.includes("w")) left += dx;
  if (edge.includes("e")) right += dx;
  if (edge.includes("n")) top += dy;
  if (edge.includes("s")) bottom += dy;

  const minWidth = 8;
  const minHeight = 6;
  if (right - left < minWidth) {
    if (edge.includes("w")) left = right - minWidth;
    else right = left + minWidth;
  }
  if (bottom - top < minHeight) {
    if (edge.includes("n")) top = bottom - minHeight;
    else bottom = top + minHeight;
  }
  return clampRect({
    x: left,
    y: top,
    width: right - left,
    height: bottom - top,
  });
}

/**
 * Re-express a floating frame after its viewport moves or resizes.
 *
 * Pixel size and screen position are preserved where possible. Exact edge
 * anchors win: a window snapped right/bottom remains snapped there when the
 * workspace sidebar takes or returns space.
 */
export function rebaseFloatingRect(
  raw: LayoutRect,
  previous: FloatingViewport,
  next: FloatingViewport,
): LayoutRect {
  const rect = clampRect(raw);
  if (
    previous.width <= 0 ||
    previous.height <= 0 ||
    next.width <= 0 ||
    next.height <= 0
  )
    return rect;

  const oldLeft = previous.left + (rect.x / 100) * previous.width;
  const oldTop = previous.top + (rect.y / 100) * previous.height;
  const oldWidth = (rect.width / 100) * previous.width;
  const oldHeight = (rect.height / 100) * previous.height;
  const oldRight = oldLeft + oldWidth;
  const oldBottom = oldTop + oldHeight;
  const previousRight = previous.left + previous.width;
  const previousBottom = previous.top + previous.height;
  const anchoredLeft = Math.abs(oldLeft - previous.left) < 0.5;
  const anchoredRight = Math.abs(oldRight - previousRight) < 0.5;
  const anchoredTop = Math.abs(oldTop - previous.top) < 0.5;
  const anchoredBottom = Math.abs(oldBottom - previousBottom) < 0.5;

  let width = oldWidth;
  let height = oldHeight;
  let left = oldLeft;
  let top = oldTop;
  if (anchoredLeft && anchoredRight) {
    left = next.left;
    width = next.width;
  } else if (anchoredLeft) {
    left = next.left;
  } else if (anchoredRight) {
    left = next.left + next.width - width;
  }
  if (anchoredTop && anchoredBottom) {
    top = next.top;
    height = next.height;
  } else if (anchoredTop) {
    top = next.top;
  } else if (anchoredBottom) {
    top = next.top + next.height - height;
  }

  const rebased = clampRect({
    x: ((left - next.left) / next.width) * 100,
    y: ((top - next.top) / next.height) * 100,
    width: (width / next.width) * 100,
    height: (height / next.height) * 100,
  });
  const clean = (value: number) => Math.round(value * 1_000_000) / 1_000_000;
  return {
    x: clean(rebased.x),
    y: clean(rebased.y),
    width: clean(rebased.width),
    height: clean(rebased.height),
  };
}

/** Human-readable title for the occupant of a floating frame. */
export function floatingWindowTitle(
  assignment: string,
  sessions: readonly YasSession[],
  surface: YasSurface | null,
): string {
  if (isTileAssignment(assignment) || isWebAssignment(assignment)) {
    const shown = tileDisplay(assignment);
    return (
      [shown.prefix, shown.title].filter(Boolean).join(" › ") ||
      shown.subtitle ||
      assignment
    );
  }
  if (surface) return surfaceName(surface);
  const session = sessions.find((candidate) => candidate.id === assignment);
  return session ? sessionName(session) : assignment;
}
