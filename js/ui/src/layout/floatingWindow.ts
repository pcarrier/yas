import type { YasSession, YasSurface } from "@yas-run/core";
import {
  cascadeRect,
  clampRect,
  enumeratePanes,
  windowManagerOf,
  type LayoutChild,
  type LayoutLeaf,
  type LayoutNode,
  type LayoutRect,
  type LayoutSplit,
} from "@yas-run/core/layout";
import { tileDisplay } from "../ide/tileDisplay";
import { sessionName, surfaceName } from "../theme";
import { isTileAssignment, isWebAssignment } from "./store";
import { removePaneFromLayout } from "./paneRemoval";

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
      !existingSurfaceKeys.has(`${surface.connectionId}:${surface.surfaceId}`),
  );
}

/** Parked/sidebar content appends a window; pane-originated content moves. */
export function floatingDropAppendsWindow(
  sourcePaneId: string | null | undefined,
): boolean {
  return sourcePaneId == null;
}

/** Whether an ordinary open belongs in a new floating frame.
 *
 * A globally floating manager always creates one frame per assignment. A
 * mixed workspace only does so for an explicit "open as floating window"
 * action; ordinary opens inherit the focused pane's tiled container instead.
 */
export function shouldOpenAsFloatingWindow(
  root: LayoutNode,
  explicit = false,
): boolean {
  return explicit || windowManagerOf(root) === "floating";
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
  if (
    root.type !== "split" ||
    (root.direction !== "floating" && root.direction !== "workspace")
  )
    return null;
  const index = root.children.length;
  const floatingIndex = root.children.filter(
    (child) => root.direction === "floating" || child.rect != null,
  ).length;
  return {
    root: {
      ...root,
      children: [
        ...root.children,
        {
          node: { type: "leaf" },
          weight: 1,
          rect: cascadeRect(floatingIndex),
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
  if (
    root.type !== "split" ||
    (root.direction !== "floating" && root.direction !== "workspace")
  )
    return null;
  const index = root.children.findIndex((child, at) => {
    const paneId = String(at);
    return (
      (root.direction === "floating" || child.rect != null) &&
      child.node.type === "leaf" &&
      assignments[paneId] == null &&
      !reservedPaneIds.has(paneId)
    );
  });
  return index < 0 ? null : String(index);
}

export interface FloatingLayoutMutation {
  root: LayoutNode;
  assignments: Record<string, string | null>;
  focusedPaneId: string;
  paneIdMap: ReadonlyMap<string, string>;
}

function panePath(root: LayoutNode, paneId: string): number[] | null {
  if (root.type === "leaf") return paneId === "0" ? [] : null;
  const path = paneId.split(".").map(Number);
  if (path.some((index) => !Number.isInteger(index) || index < 0)) return null;
  let current: LayoutNode = root;
  for (const index of path) {
    if (current.type !== "split" || !current.children[index]) return null;
    current = current.children[index].node;
  }
  return current.type === "leaf" ? path : null;
}

function leafAt(root: LayoutNode, path: readonly number[]): LayoutLeaf | null {
  let current = root;
  for (const index of path) {
    if (current.type !== "split" || !current.children[index]) return null;
    current = current.children[index].node;
  }
  return current.type === "leaf" ? current : null;
}

function valuesByLeaf(
  root: LayoutNode,
  assignments: Readonly<Record<string, string | null>>,
): Map<LayoutLeaf, string | null> {
  return new Map(
    enumeratePanes(root).map(({ id, leaf }) => [leaf, assignments[id] ?? null]),
  );
}

function finishFloatingMutation(
  oldRoot: LayoutNode,
  root: LayoutNode,
  oldAssignments: Readonly<Record<string, string | null>>,
  focusedLeaf: LayoutLeaf,
  newValues: ReadonlyMap<LayoutLeaf, string | null> = new Map(),
): FloatingLayoutMutation | null {
  const oldPanes = enumeratePanes(oldRoot);
  const oldIds = new Map(oldPanes.map(({ id, leaf }) => [leaf, id]));
  const values = valuesByLeaf(oldRoot, oldAssignments);
  const assignments: Record<string, string | null> = {};
  const paneIdMap = new Map<string, string>();
  let focusedPaneId: string | null = null;
  for (const { id, leaf } of enumeratePanes(root)) {
    assignments[id] = newValues.has(leaf)
      ? (newValues.get(leaf) ?? null)
      : (values.get(leaf) ?? null);
    if (leaf === focusedLeaf) focusedPaneId = id;
    const oldId = oldIds.get(leaf);
    if (oldId) paneIdMap.set(oldId, id);
  }
  return focusedPaneId ? { root, assignments, focusedPaneId, paneIdMap } : null;
}

function appendToTiledBase(base: LayoutNode, node: LayoutNode): LayoutNode {
  if (
    base.type === "split" &&
    base.direction !== "floating" &&
    base.direction !== "workspace"
  ) {
    return {
      ...base,
      children: [
        ...base.children,
        {
          node,
          weight: base.direction === "scrolling" ? 0.5 : 1,
        },
      ],
    };
  }
  return {
    type: "split",
    direction: "horizontal",
    children: [
      { node: base, weight: 1 },
      { node, weight: 1 },
    ],
  };
}

/** Whether a pane belongs to a Sway-style floating frame. */
export function isFloatingPane(root: LayoutNode, paneId: string): boolean {
  const path = panePath(root, paneId);
  if (!path || path.length === 0 || root.type !== "split") return false;
  if (root.direction === "floating") return true;
  return root.direction === "workspace" && root.children[path[0]]?.rect != null;
}

/** Top-level frame index for a floating pane, including nested legacy frames. */
export function floatingFrameIndex(
  root: LayoutNode,
  paneId: string,
): number | null {
  if (!isFloatingPane(root, paneId) || root.type !== "split") return null;
  const index = Number(paneId.split(".")[0]);
  return Number.isInteger(index) && root.children[index] ? index : null;
}

/** Pane ids split by Sway's tiled/floating focus modes. */
export function panesByFloatingMode(root: LayoutNode): {
  tiled: string[];
  floating: string[];
} {
  const tiled: string[] = [];
  const floating: string[] = [];
  for (const { id } of enumeratePanes(root)) {
    (isFloatingPane(root, id) ? floating : tiled).push(id);
  }
  return { tiled, floating };
}

/**
 * Toggle the focused view between the tiled tree and a floating frame.
 *
 * `workspace` is the persisted mixed scene: one rect-less base plus any
 * number of framed children. Leaf identity carries assignments across the
 * path rewrite, so toggling never creates, duplicates, or loses a surface.
 */
export function togglePaneFloating(
  root: LayoutNode,
  assignments: Readonly<Record<string, string | null>>,
  paneId: string,
  requestedRect: LayoutRect,
): FloatingLayoutMutation | null {
  const path = panePath(root, paneId);
  if (!path) return null;
  const focusedLeaf = leafAt(root, path);
  if (!focusedLeaf || !assignments[paneId]) return null;

  if (isFloatingPane(root, paneId) && root.type === "split") {
    const frameIndex = path[0];
    const frame = root.children[frameIndex];
    const remaining = root.children.filter((_, index) => index !== frameIndex);
    const baseIndex = remaining.findIndex((child) => child.rect == null);
    let base: LayoutChild;
    if (baseIndex >= 0) {
      const previous = remaining[baseIndex];
      base = {
        ...previous,
        node: appendToTiledBase(previous.node, frame.node),
      };
      remaining.splice(baseIndex, 1);
    } else {
      base = { ...frame, rect: undefined };
    }
    const floats = remaining.filter((child) => child.rect != null);
    const nextRoot: LayoutNode =
      floats.length === 0
        ? base.node
        : {
            type: "split",
            direction: "workspace",
            children: [base, ...floats],
          };
    return finishFloatingMutation(root, nextRoot, assignments, focusedLeaf);
  }

  let base: LayoutNode | null;
  let existingFloats: LayoutChild[] = [];
  if (root.type === "split" && root.direction === "workspace") {
    const baseIndex = root.children.findIndex((child) => child.rect == null);
    if (baseIndex < 0 || path[0] !== baseIndex) return null;
    const baseChild = root.children[baseIndex];
    const localPaneId = path.length === 1 ? "0" : path.slice(1).join(".");
    base = removePaneFromLayout(baseChild.node, localPaneId);
    existingFloats = root.children.filter((child) => child.rect != null);
  } else {
    base = removePaneFromLayout(root, paneId);
  }
  const children: LayoutChild[] = [];
  if (base) children.push({ node: base, weight: 1 });
  children.push(...existingFloats, {
    node: focusedLeaf,
    weight: 1,
    rect: clampRect(requestedRect),
  });
  return finishFloatingMutation(
    root,
    { type: "split", direction: "workspace", children },
    assignments,
    focusedLeaf,
  );
}

/** Add a normal tiled view when a mixed scene currently has only floats. */
export function addTiledWindowToWorkspace(
  root: LayoutNode,
  assignments: Readonly<Record<string, string | null>>,
  value: string,
): FloatingLayoutMutation | null {
  if (root.type !== "split" || root.direction !== "workspace") return null;
  if (Object.values(assignments).includes(value)) return null;
  const inserted: LayoutLeaf = { type: "leaf" };
  const baseIndex = root.children.findIndex((child) => child.rect == null);
  const children = [...root.children];
  if (baseIndex >= 0) {
    const base = children[baseIndex];
    children.splice(baseIndex, 1);
    children.unshift({ ...base, node: appendToTiledBase(base.node, inserted) });
  } else {
    children.unshift({ node: inserted, weight: 1 });
  }
  return finishFloatingMutation(
    root,
    { ...root, children },
    assignments,
    inserted,
    new Map([[inserted, value]]),
  );
}

/** Add a new independent floating view above any current manager. */
export function addFloatingWindowToWorkspace(
  root: LayoutNode,
  assignments: Readonly<Record<string, string | null>>,
  value: string,
  requestedRect: LayoutRect,
): FloatingLayoutMutation | null {
  if (Object.values(assignments).includes(value)) return null;
  const inserted: LayoutLeaf = { type: "leaf" };
  let nextRoot: LayoutNode;
  if (
    root.type === "split" &&
    (root.direction === "workspace" || root.direction === "floating")
  ) {
    nextRoot = {
      ...root,
      children: [
        ...root.children,
        {
          node: inserted,
          weight: 1,
          rect: clampRect(requestedRect),
        },
      ],
    };
  } else {
    const hasTiledContent = Object.values(assignments).some(
      (assignment) => assignment != null,
    );
    nextRoot = {
      type: "split",
      direction: "workspace",
      children: [
        ...(hasTiledContent ? [{ node: root, weight: 1 }] : []),
        {
          node: inserted,
          weight: 1,
          rect: clampRect(requestedRect),
        },
      ],
    };
  }
  return finishFloatingMutation(
    root,
    nextRoot,
    assignments,
    inserted,
    new Map([[inserted, value]]),
  );
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
        horizontalTargets(true).map((target) => target - (next.x + next.width)),
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
        verticalTargets(true).map((target) => target - (next.y + next.height)),
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
