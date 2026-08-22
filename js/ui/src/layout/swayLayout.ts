import type {
  LayoutChild,
  LayoutDirection,
  LayoutLeaf,
  LayoutNode,
  LayoutSplit,
} from "@yas-run/core/layout";
import { enumeratePanes } from "@yas-run/core/layout";
import { removePaneFromLayout } from "./paneRemoval";
import type { SpatialDirection } from "./spatialNavigation";

export type TiledLayout = "horizontal" | "vertical" | "tabs" | "stacking";

export interface LayoutMutation {
  root: LayoutNode;
  assignments: Record<string, string | null>;
  focusedPaneId: string;
  /** Every old pane id that survived, mapped to its id in the new tree. */
  paneIdMap: ReadonlyMap<string, string>;
}

function pathForPane(root: LayoutNode, paneId: string): number[] | null {
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

function pathForLeaf(
  root: LayoutNode,
  leaf: LayoutLeaf,
  path: readonly number[] = [],
): number[] | null {
  if (root.type === "leaf") return root === leaf ? [...path] : null;
  for (let index = 0; index < root.children.length; index += 1) {
    const found = pathForLeaf(root.children[index].node, leaf, [
      ...path,
      index,
    ]);
    if (found) return found;
  }
  return null;
}

function nodeAtPath(
  root: LayoutNode,
  path: readonly number[],
): LayoutNode | null {
  let current = root;
  for (const index of path) {
    if (current.type !== "split" || !current.children[index]) return null;
    current = current.children[index].node;
  }
  return current;
}

function replaceAtPath(
  root: LayoutNode,
  path: readonly number[],
  replacement: LayoutNode,
): LayoutNode {
  if (path.length === 0) return replacement;
  if (root.type !== "split") return root;
  const [head, ...rest] = path;
  return {
    ...root,
    children: root.children.map((child, index) =>
      index === head
        ? { ...child, node: replaceAtPath(child.node, rest, replacement) }
        : child,
    ),
  };
}

function paneId(path: readonly number[]): string {
  return path.length === 0 ? "0" : path.join(".");
}

function assignmentsByLeaf(
  root: LayoutNode,
  assignments: Readonly<Record<string, string | null>>,
): Map<LayoutLeaf, string | null> {
  return new Map(
    enumeratePanes(root).map(({ id, leaf }) => [leaf, assignments[id] ?? null]),
  );
}

/**
 * Re-key assignments after an immutable tree transformation. Pane ids are
 * paths and therefore disposable; leaf identity is the stable bridge across
 * the edit. New leaves may supply their initial assignment in `newValues`.
 */
function finishMutation(
  oldRoot: LayoutNode,
  nextRoot: LayoutNode,
  oldAssignments: Readonly<Record<string, string | null>>,
  focusedLeaf: LayoutLeaf,
  newValues: ReadonlyMap<LayoutLeaf, string | null> = new Map(),
): LayoutMutation | null {
  const oldPanes = enumeratePanes(oldRoot);
  const oldIds = new Map(oldPanes.map(({ id, leaf }) => [leaf, id]));
  const oldValues = assignmentsByLeaf(oldRoot, oldAssignments);
  const assignments: Record<string, string | null> = {};
  const paneIdMap = new Map<string, string>();
  let focusedPaneId: string | null = null;

  for (const { id, leaf } of enumeratePanes(nextRoot)) {
    assignments[id] = newValues.has(leaf)
      ? (newValues.get(leaf) ?? null)
      : (oldValues.get(leaf) ?? null);
    if (leaf === focusedLeaf) focusedPaneId = id;
    const oldId = oldIds.get(leaf);
    if (oldId) paneIdMap.set(oldId, id);
  }

  return focusedPaneId
    ? { root: nextRoot, assignments, focusedPaneId, paneIdMap }
    : null;
}

function isTiledSplit(node: LayoutNode | null): node is LayoutSplit {
  return node?.type === "split" && node.direction !== "workspace";
}

/**
 * Put a new populated container after `targetPaneId`.
 *
 * Matching parent splits are extended instead of producing the staircase of
 * redundant two-child wrappers that a naive BSP insertion creates. A
 * different parent orientation is preserved by nesting at the focused leaf,
 * exactly like sway's split containers.
 */
export function splitPaneWithAssignment(
  root: LayoutNode,
  assignments: Readonly<Record<string, string | null>>,
  targetPaneId: string,
  value: string,
  direction: TiledLayout,
  placeAfter = true,
): LayoutMutation | null {
  if (
    Object.entries(assignments).some(
      ([id, assignment]) => id !== targetPaneId && assignment === value,
    )
  ) {
    return null;
  }
  const path = pathForPane(root, targetPaneId);
  if (!path) return null;
  const target = nodeAtPath(root, path);
  if (target?.type !== "leaf") return null;

  const inserted: LayoutLeaf = { type: "leaf" };
  let nextRoot: LayoutNode;
  if (path.length > 0) {
    const parentPath = path.slice(0, -1);
    const parent = nodeAtPath(root, parentPath);
    if (parent?.type === "split" && parent.direction === direction) {
      const at = path[path.length - 1] + (placeAfter ? 1 : 0);
      const children = [...parent.children];
      children.splice(at, 0, { node: inserted, weight: 1 });
      nextRoot = replaceAtPath(root, parentPath, { ...parent, children });
    } else {
      const children: [LayoutChild, LayoutChild] = placeAfter
        ? [
            { node: target, weight: 1 },
            { node: inserted, weight: 1 },
          ]
        : [
            { node: inserted, weight: 1 },
            { node: target, weight: 1 },
          ];
      nextRoot = replaceAtPath(root, path, {
        type: "split",
        direction,
        children,
      });
    }
  } else {
    const children: [LayoutChild, LayoutChild] = placeAfter
      ? [
          { node: target, weight: 1 },
          { node: inserted, weight: 1 },
        ]
      : [
          { node: inserted, weight: 1 },
          { node: target, weight: 1 },
        ];
    nextRoot = {
      type: "split",
      direction,
      children,
    };
  }
  return finishMutation(
    root,
    nextRoot,
    assignments,
    inserted,
    new Map([[inserted, value]]),
  );
}

/** Change the focused container's current child layout. */
export function setPaneLayout(
  root: LayoutNode,
  assignments: Readonly<Record<string, string | null>>,
  focusedPaneId: string,
  direction: TiledLayout,
): LayoutMutation | null {
  const path = pathForPane(root, focusedPaneId);
  if (!path || path.length === 0) return null;
  const leaf = nodeAtPath(root, path);
  const parentPath = path.slice(0, -1);
  const parent = nodeAtPath(root, parentPath);
  if (leaf?.type !== "leaf" || !isTiledSplit(parent)) return null;
  if (parent.direction === direction) return null;
  return finishMutation(
    root,
    replaceAtPath(root, parentPath, { ...parent, direction }),
    assignments,
    leaf,
  );
}

/** Toggle the focused container between horizontal and vertical splitting. */
export function togglePaneSplit(
  root: LayoutNode,
  assignments: Readonly<Record<string, string | null>>,
  focusedPaneId: string,
): LayoutMutation | null {
  const path = pathForPane(root, focusedPaneId);
  if (!path || path.length === 0) return null;
  const parent = nodeAtPath(root, path.slice(0, -1));
  const direction =
    parent?.type === "split" && parent.direction === "horizontal"
      ? "vertical"
      : "horizontal";
  return setPaneLayout(root, assignments, focusedPaneId, direction);
}

function axisFor(direction: SpatialDirection): "horizontal" | "vertical" {
  return direction === "left" || direction === "right"
    ? "horizontal"
    : "vertical";
}

function isLeading(direction: SpatialDirection): boolean {
  return direction === "left" || direction === "up";
}

function insertLeafBeside(
  root: LayoutNode,
  target: LayoutLeaf,
  source: LayoutLeaf,
  direction: SpatialDirection,
): LayoutNode | null {
  const targetPath = pathForLeaf(root, target);
  if (!targetPath) return null;
  const axis = axisFor(direction);
  const leading = isLeading(direction);
  if (targetPath.length > 0) {
    const parentPath = targetPath.slice(0, -1);
    const parent = nodeAtPath(root, parentPath);
    if (parent?.type === "split" && parent.direction === axis) {
      const children = [...parent.children];
      const targetIndex = targetPath[targetPath.length - 1];
      children.splice(targetIndex + (leading ? 0 : 1), 0, {
        node: source,
        weight: 1,
      });
      return replaceAtPath(root, parentPath, { ...parent, children });
    }
  }
  const children: [LayoutChild, LayoutChild] = leading
    ? [
        { node: source, weight: 1 },
        { node: target, weight: 1 },
      ]
    : [
        { node: target, weight: 1 },
        { node: source, weight: 1 },
      ];
  return replaceAtPath(root, targetPath, {
    type: "split",
    direction: axis,
    children,
  });
}

/**
 * Move the focused sway container in a visual direction.
 *
 * Adjacent siblings swap directly. Otherwise the source leaf is removed and
 * inserted beside the geometric target; at an outer edge it becomes a new
 * leading/trailing root child. This is structural movement, not tab grouping.
 */
export function movePaneInDirection(
  root: LayoutNode,
  assignments: Readonly<Record<string, string | null>>,
  sourcePaneId: string,
  targetPaneId: string | null,
  direction: SpatialDirection,
): LayoutMutation | null {
  const sourcePath = pathForPane(root, sourcePaneId);
  if (!sourcePath || !assignments[sourcePaneId]) return null;
  const source = nodeAtPath(root, sourcePath);
  if (source?.type !== "leaf") return null;
  let targetPath = targetPaneId ? pathForPane(root, targetPaneId) : null;
  let target = targetPath ? nodeAtPath(root, targetPath) : null;
  if (target === source) {
    // Dragging the active tab onto its own content edge reports the same pane
    // as source and target. Use another leaf from that explicit tab/stack
    // container as the anchor so the source can be pulled back into a split.
    const parent = nodeAtPath(root, sourcePath.slice(0, -1));
    if (
      parent?.type !== "split" ||
      (parent.direction !== "tabs" && parent.direction !== "stacking")
    ) {
      return null;
    }
    target =
      enumeratePanes(parent).find(({ leaf }) => leaf !== source)?.leaf ?? null;
    targetPath = target ? pathForLeaf(root, target) : null;
  }
  if (targetPaneId && target?.type !== "leaf") return null;

  const axis = axisFor(direction);
  if (
    targetPath &&
    sourcePath.length === targetPath.length &&
    sourcePath.length > 0 &&
    sourcePath.slice(0, -1).join(".") === targetPath.slice(0, -1).join(".")
  ) {
    const parentPath = sourcePath.slice(0, -1);
    const parent = nodeAtPath(root, parentPath);
    if (parent?.type === "split" && parent.direction === axis) {
      const sourceIndex = sourcePath[sourcePath.length - 1];
      const targetIndex = targetPath[targetPath.length - 1];
      const children = [...parent.children];
      [children[sourceIndex], children[targetIndex]] = [
        children[targetIndex],
        children[sourceIndex],
      ];
      return finishMutation(
        root,
        replaceAtPath(root, parentPath, { ...parent, children }),
        assignments,
        source,
      );
    }
  }

  const withoutSource = removePaneFromLayout(root, sourcePaneId);
  if (!withoutSource) return null;
  let nextRoot: LayoutNode | null;
  if (target?.type === "leaf") {
    nextRoot = insertLeafBeside(withoutSource, target, source, direction);
  } else {
    const sourceChild = { node: source, weight: 1 };
    const remainingChild = { node: withoutSource, weight: 1 };
    nextRoot = {
      type: "split",
      direction: axis,
      children: isLeading(direction)
        ? [sourceChild, remainingChild]
        : [remainingChild, sourceChild],
    };
  }
  return nextRoot ? finishMutation(root, nextRoot, assignments, source) : null;
}

/** Layouts available when cycling a tiled container. */
export function nextTiledLayout(direction: LayoutDirection): TiledLayout {
  if (direction === "horizontal") return "vertical";
  if (direction === "vertical") return "tabs";
  if (direction === "tabs") return "stacking";
  return "horizontal";
}

export function paneParentLayout(
  root: LayoutNode,
  focusedPaneId: string,
): LayoutDirection | null {
  const path = pathForPane(root, focusedPaneId);
  if (!path || path.length === 0) return null;
  const parent = nodeAtPath(root, path.slice(0, -1));
  return parent?.type === "split" ? parent.direction : null;
}

export const _test = { pathForPane, pathForLeaf, nodeAtPath, paneId };
